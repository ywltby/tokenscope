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
use crate::retry::HISTORY_BUSY_TIMEOUT;

/// 当前库结构版本（`PRAGMA user_version`）。
///
/// - v1 = `history_meta` + `source_files` + `usage_events`；
/// - v2 = 别名 / 来源记录 / CCS 日汇总 / 导入批次四表（H01 起）。
pub const HISTORY_SCHEMA_VERSION: i64 = 4;

/// 适配器输出语义版本：标记新写入的事件/来源并进入发行指纹，不触发结构迁移。
/// 旧文件重解析由 report::PARSE_RULES_REVISION 驱动，未纠正的事实保留原版本标记。
/// v2：Codex 压缩请求配对与线程设置模型归属。
pub const PARSER_REVISION: i64 = 2;

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

/// v2 建表 SQL（迁移使用；集成测试据此构造 v2 旧库验证 v3 数据迁移）。
#[doc(hidden)]
pub fn legacy_v2_schema_for_tests() -> &'static str {
    SCHEMA_V2
}

/// v3 数据迁移：修复三个"旧库升级后行为不正确"的数据问题（结构不变，只改数据）。
///
/// - **日汇总身份去掉 `source_tz`**（v2 把它放进了唯一键）：同一 CCS 库改选
///   来源时区再次导入时，同一份日汇总获得了两个身份、统计重复累计。重建表并把
///   仅时区不同的重复行折叠为最早一条（同键快照冲突一律保留先存值，与运行期
///   KeepExisting 策略一致）；CCS 的完整主键本就不含时区（计划 §5.3）。
/// - **观察时间改写为事件时间**：v2 及更早版本把"采集时刻"存进
///   `observed_at`，升级后代码按"事件时间"判定终值新旧——旧记录的采集时刻
///   总是晚于其事件时间，后续真实终值会被误判为过期。全部改写为
///   `ts_seconds/ts_nanos`（CCS 行本来就存事件时间，改写是无操作）。
/// - **补齐 Codex 保守重播身份别名**：v2 时代的原生事件没有 `codex-usage`
///   别名，而已采集文件的指纹命中会让重解析不再发生——文件一旦移动到归档
///   目录就找不到旧事件、重复计量。从库内字段重建该别名（与
///   `UsageEvent::same_source_identity` 同一构造规则）；同身份对应多个事件时
///   无法唯一改绑，保持缺失（这些事件维持既有保守行为）。
const SCHEMA_V3: &str = r"
CREATE TABLE ccs_daily_usage_v3 (
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
    UNIQUE(logical_source, day, app, provider_id, model, request_model, pricing_model)
);
INSERT INTO ccs_daily_usage_v3(logical_source, day, source_tz, app, provider_id, model,
    request_model, pricing_model, request_count, input_tokens, output_tokens,
    cache_write_tokens, cache_read_tokens, input_semantics, source_cost_usd,
    import_run_id, revision)
SELECT logical_source, day, source_tz, app, provider_id, model, request_model, pricing_model,
    request_count, input_tokens, output_tokens, cache_write_tokens, cache_read_tokens,
    input_semantics, source_cost_usd, import_run_id, revision
FROM ccs_daily_usage c
WHERE c.id = (
    SELECT MIN(c2.id) FROM ccs_daily_usage c2
    WHERE c2.logical_source = c.logical_source AND c2.day = c.day AND c2.app = c.app
      AND c2.provider_id = c.provider_id AND c2.model = c.model
      AND c2.request_model = c.request_model AND c2.pricing_model = c.pricing_model
);
DROP TABLE ccs_daily_usage;
ALTER TABLE ccs_daily_usage_v3 RENAME TO ccs_daily_usage;
CREATE INDEX IF NOT EXISTS idx_ccs_daily_day ON ccs_daily_usage(day, app, model);
UPDATE usage_events SET observed_at_seconds = ts_seconds, observed_at_nanos = ts_nanos,
    -- observed_at_utc 与运行期写入同格式（RFC3339，jiff Timestamp Display）；
    -- 亚秒不进 utc 文本（真值保留在 observed_at_nanos），迁移纯 SQL 不做逐行分支。
    observed_at_utc = strftime('%Y-%m-%dT%H:%M:%SZ', ts_seconds, 'unixepoch');
INSERT INTO event_aliases(event_id, app, identity_scheme, identity_value, created_generation)
SELECT e.id, 'codex', 'codex-usage',
       e.session_id || '|' || e.model_raw || '|' || e.input_tokens || '|' || e.output_tokens
         || '|' || e.cache_write_tokens || '|' || e.cache_read_tokens,
       0
FROM usage_events e
WHERE e.app = 'codex'
  AND COALESCE(e.session_id, '') <> ''
  AND e.model_raw <> ''
  AND NOT EXISTS (
      SELECT 1 FROM event_aliases x
      WHERE x.event_id = e.id AND x.identity_scheme = 'codex-usage')
  AND (
      SELECT COUNT(*) FROM usage_events g
      WHERE g.app = 'codex' AND COALESCE(g.session_id, '') <> '' AND g.model_raw <> ''
        AND g.session_id = e.session_id AND g.model_raw = e.model_raw
        AND g.input_tokens = e.input_tokens AND g.output_tokens = e.output_tokens
        AND g.cache_write_tokens = e.cache_write_tokens
        AND g.cache_read_tokens = e.cache_read_tokens
  ) = 1
  AND NOT EXISTS (
      SELECT 1 FROM event_aliases y
      WHERE y.app = 'codex' AND y.identity_scheme = 'codex-usage'
        AND y.identity_value = e.session_id || '|' || e.model_raw || '|' || e.input_tokens
          || '|' || e.output_tokens || '|' || e.cache_write_tokens || '|' || e.cache_read_tokens
  );
";

/// 版本化迁移表：`(目标版本, SQL)`，从 0 起按序执行。
/// v4 数据迁移：合并被旧实现遗留的 **Codex 重复事件**（路径来源键时代的产物）。
///
/// v2 及更早版本的采集身份是"文件路径 + 行序 + 内容指纹"——同一请求的副本
/// （归档移动、后到副本、迁移后重扫）因来源键不同被各自插入，而当时的解析
/// 又不写保守重播身份别名，这些重复行此后无法被任何一次重解析命中。合并
/// 规则与 `dedupe` §3.2 的保守重播规则完全一致：同 `(session, 原始模型, 四桶)`
/// 视为同一次请求（该口径的"同模型同用量独立请求被保守合并"限制已在
/// stats-semantics §3.2 披露，迁移沿用同一限制，不新增猜测）。
///
/// 组内保留一条事实承载行：原生优先（`origin_rank` 高者）、时间最早、行号
/// 最小（与重播规则"保留首条"一致）；其余行的来源证据与别名改挂到保留行后
/// 删除，并为保留行补齐 `codex-usage` 别名——此后的重解析与 CCS 导入命中
/// 同一事件，重复不再增长。可区分的独立请求（不同会话/模型/四桶）不参与
/// 合并，事实一律保留。
const SCHEMA_V4: &str = r"
CREATE TEMP TABLE v4_keeper AS
SELECT id AS keeper_id,
       session_id || '|' || model_raw || '|' || input_tokens || '|' || output_tokens
         || '|' || cache_write_tokens || '|' || cache_read_tokens AS grp
FROM (
    SELECT id, session_id, model_raw, input_tokens, output_tokens,
           cache_write_tokens, cache_read_tokens,
           ROW_NUMBER() OVER (
               PARTITION BY session_id, model_raw, input_tokens, output_tokens,
                            cache_write_tokens, cache_read_tokens
               ORDER BY origin_rank DESC, ts_seconds ASC, ts_nanos ASC, id ASC
           ) AS rn
    FROM usage_events
    WHERE app = 'codex' AND COALESCE(session_id, '') <> '' AND model_raw <> ''
)
WHERE rn = 1;

CREATE TEMP TABLE v4_loser AS
SELECT e.id,
       e.session_id || '|' || e.model_raw || '|' || e.input_tokens || '|' || e.output_tokens
         || '|' || e.cache_write_tokens || '|' || e.cache_read_tokens AS grp
FROM usage_events e
WHERE e.app = 'codex' AND COALESCE(e.session_id, '') <> '' AND e.model_raw <> ''
  AND NOT EXISTS (SELECT 1 FROM v4_keeper k WHERE k.keeper_id = e.id)
  AND EXISTS (
      SELECT 1 FROM usage_events g
      WHERE g.app = 'codex' AND g.id <> e.id
        AND COALESCE(g.session_id, '') = e.session_id AND g.model_raw = e.model_raw
        AND g.input_tokens = e.input_tokens AND g.output_tokens = e.output_tokens
        AND g.cache_write_tokens = e.cache_write_tokens
        AND g.cache_read_tokens = e.cache_read_tokens
  );

UPDATE event_aliases
SET event_id = (SELECT k.keeper_id FROM v4_loser l JOIN v4_keeper k ON k.grp = l.grp
                WHERE l.id = event_aliases.event_id)
WHERE event_id IN (SELECT id FROM v4_loser);

UPDATE event_origins
SET event_id = (SELECT k.keeper_id FROM v4_loser l JOIN v4_keeper k ON k.grp = l.grp
                WHERE l.id = event_origins.event_id)
WHERE event_id IN (SELECT id FROM v4_loser);

DELETE FROM usage_events WHERE id IN (SELECT id FROM v4_loser);

INSERT INTO event_aliases(event_id, app, identity_scheme, identity_value, created_generation)
SELECT k.keeper_id, 'codex', 'codex-usage', k.grp, 0
FROM v4_keeper k
WHERE NOT EXISTS (
      SELECT 1 FROM event_aliases a
      WHERE a.event_id = k.keeper_id AND a.identity_scheme = 'codex-usage')
  AND NOT EXISTS (
      SELECT 1 FROM event_aliases a
      WHERE a.app = 'codex' AND a.identity_scheme = 'codex-usage'
        AND a.identity_value = k.grp);

DROP TABLE v4_loser;
DROP TABLE v4_keeper;
";

const MIGRATIONS: &[(i64, &str)] = &[
    (1, SCHEMA_V1),
    (2, SCHEMA_V2),
    (3, SCHEMA_V3),
    (4, SCHEMA_V4),
];

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
    /// 自动插入的新请求中，存在"同时间同用量但身份不足"重叠候选的记录数
    /// （仅供审计：调用方决定是否允许这类导入）。
    pub overlap_candidates: u64,
    /// 新增的别名行数。
    pub aliases_added: u64,
    /// 新增的来源证据行数。
    pub origins_added: u64,
    /// **本批插入**事件的四桶之和：只给已有事件补来源证据时保持 0
    ///（净新增用量必须按真正新增的事实统计）。
    pub inserted_tokens: TokenCounts,
}

impl WriteSummary {
    /// 本批次是否改变了用量行（插入或更新）。
    pub fn changed_events(&self) -> u64 {
        self.inserted + self.updated
    }
}

/// 原生采集结果：升级纠正的诊断只存在于本轮，不写入持久计数或查询 DTO。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CollectedWriteSummary {
    pub events: WriteSummary,
    pub models_corrected: u64,
    pub ambiguous_models: u64,
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
    /// **重叠候选未解决**：来源记录没有可证实身份，但库内存在"同应用、同模型
    /// 等价身份、四桶相同、时间接近"的既有请求。时间与用量相同只是核对线索，
    /// 不能据此宣称是同一次请求——默认按冲突呈现并要求用户显式决定
    ///（计划 §5.3：缺少跨来源身份且存在重叠候选时，预览列为未解决，
    /// 默认不提交含此类冲突的计划）。
    OverlapCandidate,
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
    /// **不是身份的一部分**：同一 CCS 库改选时区再次导入更新该假设，不产生
    /// 第二份快照。
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

/// H07：流式读取的一行轻量索引（不含事件本体，避免整表物化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRef {
    pub id: i64,
    pub ts: Timestamp,
    pub record_id: String,
}

/// H07：只读快照——独立只读连接 + 显式读事务。
///
/// WAL 模式下，读事务在第一次读取时建立一致视图：采集或导入提交之后，**旧
/// 查询看到的仍是事务开始时的数据**（不静默换页、不混入新数据）；快照被淘汰
/// 或超过闲置期限时随连接一起释放，读事务不再长期阻塞 WAL checkpoint。
/// 连接与写连接分离，因此快照不占用写入方的锁。
pub struct ReadSnapshot {
    /// `rusqlite::Connection` 是 `Send` 但不是 `Sync`：查询会话可能被多个线程
    /// 读取（汇总/分页），因此连接放在互斥量后面——同一时刻一个读者，语义不变。
    conn: std::sync::Mutex<Connection>,
    closed: bool,
}

/// 当前存活的只读快照数（诊断与测试：验证读事务随会话淘汰/超时释放）。
pub fn open_read_snapshot_count() -> usize {
    OPEN_READ_SNAPSHOTS.load(std::sync::atomic::Ordering::Relaxed)
}

static OPEN_READ_SNAPSHOTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl std::fmt::Debug for ReadSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadSnapshot")
            .field("closed", &self.closed)
            .finish()
    }
}

impl HistoryDb {
    /// 建立一个只读快照（独立连接，不阻塞写入连接）。
    pub fn read_snapshot(&self) -> Result<ReadSnapshot> {
        let conn = Connection::open_with_flags(
            &self.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("打开历史库只读快照失败: {}", self.path.display()))?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        // 显式读事务：视图在第一次读取时固定，之后不再变化（WAL 快照隔离）。
        conn.execute_batch("BEGIN")
            .context("开启历史库只读事务失败")?;
        OPEN_READ_SNAPSHOTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(ReadSnapshot {
            conn: std::sync::Mutex::new(conn),
            closed: false,
        })
    }
}

impl ReadSnapshot {
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 快照内的用量/身份 generation（冻结视图，不随新提交改变）。
    pub fn generation(&self) -> Result<u64> {
        match self.meta(META_GENERATION)? {
            Some(v) => parse_u64_text(&v, META_GENERATION),
            None => Ok(0),
        }
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT value FROM history_meta WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    }

    /// 快照内的事件总数（`None` = 全部应用）。
    pub fn event_count(&self, app: Option<AgentKind>) -> Result<u64> {
        let n: i64 = self.lock().query_row(
            "SELECT COUNT(*) FROM usage_events WHERE (?1 IS NULL OR app = ?1)",
            params![app.map(|a| a.as_str())],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u64)
    }

    /// 日汇总快照（按时间/应用过滤在调用方完成）。
    pub fn rollups(&self, app: Option<AgentKind>) -> Result<Vec<StoredRollup>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT logical_source, day, source_tz, app, provider_id, model, request_model,
                    pricing_model, request_count, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens, input_semantics, source_cost_usd
             FROM ccs_daily_usage
             WHERE (?1 IS NULL OR app = ?1)
             ORDER BY day, app, model, provider_id",
        )?;
        let rows = stmt.query_map(params![app.map(|a| a.as_str())], |r| {
            let raw_app: String = r.get(3)?;
            let app = parse_agent(&raw_app).map_err(|e| conversion_error(3, e))?;
            Ok(StoredRollup {
                logical_source: r.get(0)?,
                day: r.get(1)?,
                source_tz: r.get(2)?,
                app,
                provider_id: r.get(4)?,
                model: r.get(5)?,
                request_model: r.get(6)?,
                pricing_model: r.get(7)?,
                request_count: r.get::<_, i64>(8)?.max(0) as u64,
                tokens: TokenCounts {
                    input: u64_from_row(r, 9)?,
                    output: u64_from_row(r, 10)?,
                    cache_write: u64_from_row(r, 11)?,
                    cache_read: u64_from_row(r, 12)?,
                },
                input_semantics: r.get(13)?,
                source_cost_usd: r
                    .get::<_, Option<String>>(14)?
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .filter(|v| v.is_finite()),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// **分批**流式遍历行索引（keyset 分页）：每批在**锁内**只取数据，
    /// 回调在**锁外**执行——回调因此可以安全地再次读取本快照（早期实现
    /// 在持锁期间回调，`fetch_events` 会重复获取同一把非重入互斥量而死锁）。
    pub fn for_each_row_batch(
        &self,
        app: Option<AgentKind>,
        batch: usize,
        f: &mut dyn FnMut(&[RowRef]) -> Result<()>,
    ) -> Result<()> {
        let batch = batch.max(1) as i64;
        let mut after: Option<(i64, i64, i64)> = None;
        loop {
            let rows = {
                let conn = self.lock();
                let mut stmt = conn.prepare(
                    "SELECT id, ts_seconds, ts_nanos, record_id
                     FROM usage_events
                     WHERE (?1 IS NULL OR app = ?1)
                       AND (?2 IS NULL OR (ts_seconds, ts_nanos, id) > (?2, ?3, ?4))
                     ORDER BY ts_seconds, ts_nanos, id
                     LIMIT ?5",
                )?;
                let (s, n, i) = match after {
                    Some(v) => (Some(v.0), Some(v.1), Some(v.2)),
                    None => (None, None, None),
                };
                let mapped =
                    stmt.query_map(params![app.map(|a| a.as_str()), s, n, i, batch], |row| {
                        let id: i64 = row.get(0)?;
                        let secs: i64 = row.get(1)?;
                        let nanos: i64 = row.get(2)?;
                        let record_id: Option<String> = row.get(3)?;
                        Ok(RowRef {
                            id,
                            ts: timestamp_from_parts(secs, nanos),
                            record_id: record_id.unwrap_or_default(),
                        })
                    })?;
                let mut out = Vec::new();
                for r in mapped {
                    out.push(r?);
                }
                out
            };
            if rows.is_empty() {
                return Ok(());
            }
            after = rows
                .last()
                .map(|r| (r.ts.as_second(), r.ts.subsec_nanosecond() as i64, r.id));
            f(&rows)?;
        }
    }

    /// **流式**遍历行的轻量索引，顺序固定为 `(ts_seconds, ts_nanos, id)` 升序。
    /// 逐行回调（内部按批读取，锁不跨越回调）。
    pub fn stream_rows(
        &self,
        app: Option<AgentKind>,
        f: &mut dyn FnMut(RowRef) -> Result<()>,
    ) -> Result<()> {
        self.for_each_row_batch(app, 1024, &mut |rows| {
            for r in rows {
                f(r.clone())?;
            }
            Ok(())
        })
    }

    /// **流式**分批读取事件本体：每批至多 `chunk` 条，内存占用与批大小成正比。
    pub fn for_each_chunk(
        &self,
        app: Option<AgentKind>,
        chunk: usize,
        f: &mut dyn FnMut(&[StoredEvent]) -> Result<()>,
    ) -> Result<()> {
        self.for_each_row_batch(app, chunk, &mut |rows| {
            let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
            // 锁已释放：`fetch_events` 在这里取锁不会与行扫描互锁。
            let events = self.fetch_events(&ids)?;
            f(&events)
        })
    }

    /// 按 id 批量取事件本体（排序按库内 `(ts, id)`，调用方按需重排）。
    pub fn fetch_events(&self, ids: &[i64]) -> Result<Vec<StoredEvent>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        // id 是库内整数主键：拼占位符而不是插值，避免 SQL 注入面。
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, event_key, app, ts_seconds, ts_nanos, model_raw, model_identity,
                    identity_revision, session_id, record_id, project_key,
                    session_initial_cwd, event_cwd, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens, origin_rank, observed_at_seconds,
                    observed_at_nanos, parser_revision
             FROM usage_events WHERE id IN ({placeholders})"
        );
        let conn = self.lock();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), row_to_stored_event)?;
        let mut out = Vec::with_capacity(ids.len());
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 释放读事务（闲置淘汰、TTL 到期或显式关闭时调用；Drop 时兜底）。
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        OPEN_READ_SNAPSHOTS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        // 只读事务用 ROLLBACK 结束：不产生写入，也不留下未提交状态。
        if let Err(e) = self.lock().execute_batch("ROLLBACK") {
            log::debug!("历史库只读快照结束事务失败（连接即将释放）: {e}");
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

impl Drop for ReadSnapshot {
    fn drop(&mut self) {
        self.close();
    }
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

/// H06：日汇总快照的读取形态（只有日粒度，没有逐请求时间/会话/cwd）。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredRollup {
    pub logical_source: String,
    /// CCS 原始统计日（来源时区的自然日，不重切时区）。
    pub day: String,
    pub source_tz: String,
    pub app: AgentKind,
    pub provider_id: String,
    pub model: String,
    pub request_model: String,
    pub pricing_model: String,
    pub request_count: u64,
    pub tokens: TokenCounts,
    pub input_semantics: i64,
    pub source_cost_usd: Option<f64>,
}

/// TokenScope 数据目录下的历史库路径（`~/.tokenscope/history.db`）。
pub fn history_file_path() -> Result<PathBuf> {
    Ok(crate::report::data_dir()?.join("history.db"))
}

// 迁移路径的检查点轨迹（仅测试）：验证「备份完成后仍会迁移」与「迁移
// 事务使用固定锁等待值」，不改变生产行为。
#[cfg(test)]
thread_local! {
    static MIGRATE_TRACE: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn trace(msg: String) {
    MIGRATE_TRACE.with(|t| t.borrow_mut().push(msg));
}

#[cfg(test)]
fn take_trace() -> Vec<String> {
    MIGRATE_TRACE.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

/// 迁移失败 + 数据位置说明。
///
/// 两个约束同时成立：
/// - `Display` 自带完整原因（"执行 v1 迁移失败: ...（历史库文件保留在 X，可
///   重试；不执行删表重建）"）——调用方只用单层 `to_string()` 也必须看到未知
///   版本 / SQL 失败，不得被位置说明压成一句无信息量的话；
/// - `source` 保留原始类型化错误链——打开层重试要按 `rusqlite::Error` 类型
///   判定瞬态错误，不能用 `anyhow!("{:#}")` 把类型字符串化。
#[derive(Debug)]
struct MigrateFailure {
    detail: String,
    source: anyhow::Error,
}

impl std::fmt::Display for MigrateFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for MigrateFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

/// 打开/创建历史库（含版本迁移）。未知的更高版本一律报错且保留文件。
#[derive(Debug)]
pub struct HistoryDb {
    conn: Connection,
    path: PathBuf,
}

impl HistoryDb {
    /// 并发首次初始化：另一个连接可能正在同一新建库上做 WAL 切换/迁移，
    /// 本连接会拿到 SQLITE_READONLY（2026-10-19 探针：30 轮里 11 轮失败，
    /// 9 次在 v1 迁移的 execute_batch、2 次在 WAL 切换；预建库后并发打开、
    /// 保持连接存活的并发打开、串行打开三组对照均 30/30 通过）。
    ///
    /// 这类错误**不能靠同一连接重试**：探针实测同一连接内重试 migrate
    /// 20 次仍失败（连接级 journal 状态与库头不一致），重新打开连接才恢复。
    /// 因此在这里重试整个打开序列（每次新建连接）；绝对 deadline 只在本层
    /// 创建一次，只约束失败后的下一次重试，不中断成功的打开或迁移。
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_clock(path, &crate::retry::RealClock)
    }

    /// 生产打开序列（时钟可注入：生产用 [`crate::retry::RealClock`]；测试用
    /// 步进假时钟模拟「两个预算检查点之间发生真实 I/O（备份）消耗」，从而
    /// 覆盖 `open → open_once → migrate` 的**生产接线**）。
    fn open_with_clock(path: &Path, clock: &dyn crate::retry::Clock) -> Result<Self> {
        #[cfg(feature = "acceptance")]
        crate::acceptance::record_path("history_open", path, serde_json::json!({}))?;
        let deadline =
            crate::retry::Deadline::new(clock, crate::retry::CONCURRENT_INIT_RETRY_BUDGET);
        crate::retry::retry_until(
            &deadline,
            clock,
            crate::retry::CONCURRENT_INIT_RETRY_BACKOFF,
            || Self::open_once(path, &deadline, clock),
        )
    }

    fn open_once(
        path: &Path,
        deadline: &crate::retry::Deadline,
        clock: &dyn crate::retry::Clock,
    ) -> Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("创建历史库目录失败: {}", dir.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("打开历史库失败: {}", path.display()))?;
        // WAL 切换需要短暂独占，且 SQLite 不为该切换调用 busy handler；
        // **每次尝试内部**都恢复固定 busy_timeout，再在同一个
        // 绝对 deadline 内小步重试（内层不重算预算）。并发**首次**初始化时
        // 另一连接可能正在切换 WAL，本连接会拿到 SQLITE_READONLY
        //（2026-10-19 探针：2/30 轮），与 DatabaseBusy 同一判定。
        crate::retry::retry_until(
            deadline,
            clock,
            crate::retry::CONCURRENT_INIT_RETRY_BACKOFF,
            || {
                crate::retry::apply_busy_timeout(&conn, HISTORY_BUSY_TIMEOUT)?;
                Ok(conn.pragma_update(None, "journal_mode", "WAL")?)
            },
        )
        .context("设置历史库 WAL 日志模式失败")?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .context("启用历史库外键约束失败")?;
        // 持久事实存储：提交即落盘（WAL + FULL），不因崩溃丢已确认的用量。
        conn.pragma_update(None, "synchronous", "FULL")
            .context("设置历史库同步级别失败")?;
        let db = Self {
            conn,
            path: path.to_path_buf(),
        };
        // 迁移失败：Display 带出完整原因与保留位置，source 保留类型化错误链
        //（打开层重试按类型判定瞬态错误，不得字符串化）。
        db.migrate().map_err(|e| {
            anyhow::Error::new(MigrateFailure {
                detail: format!(
                    "{:#}（历史库文件保留在 {}，可重试；不执行删表重建）",
                    e,
                    path.display()
                ),
                source: e,
            })
        })?;
        crate::retry::apply_busy_timeout(&db.conn, HISTORY_BUSY_TIMEOUT)?;
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

    /// 版本迁移：**先备份**（SQLite 在线备份语义的 `VACUUM INTO`，生成可直接
    /// 打开的完整副本），再在事务内按序执行迁移，失败整体回滚并保留原库。
    ///
    /// 备份只在实际发生升级（`0 < 现有版本 < 当前版本`）时生成，且已存在则
    /// 不覆盖——用户总能在 TokenScope 数据目录里找到升级前的可恢复副本
    ///（计划 §6：变更前通过 SQLite 备份 API 在 TokenScope 目录生成可恢复备份）。
    ///
    /// 读版本、备份和迁移事务使用固定锁等待值。备份与 DDL 耗时不受
    /// 重试截止时间限制；只有瞬态失败后的重试需要检查该截止时间。
    fn migrate(&self) -> Result<()> {
        // 读版本前恢复常驻锁等待值。
        crate::retry::apply_busy_timeout(&self.conn, HISTORY_BUSY_TIMEOUT)?;
        let current = self.schema_version()?;
        if current > HISTORY_SCHEMA_VERSION {
            bail!(
                "历史库结构版本 {current} 高于本程序支持的 {HISTORY_SCHEMA_VERSION}；\
                 保留文件不作修改，请升级 TokenScope 后再打开"
            );
        }
        // 最新结构且必要元信息完整时只读返回，不争抢业务写锁。
        if current == HISTORY_SCHEMA_VERSION {
            let present: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM history_meta WHERE key IN (?1, ?2, ?3, ?4)",
                params![
                    META_GENERATION,
                    META_PARSER_REVISION,
                    META_IDENTITY_REVISION,
                    META_CREATED_UTC
                ],
                |row| row.get(0),
            )?;
            if present == 4 {
                return Ok(());
            }
        }
        if current > 0 && current < HISTORY_SCHEMA_VERSION {
            // 升级前先落可恢复备份（失败即中止本次升级，结构不变）。
            crate::retry::apply_busy_timeout(&self.conn, HISTORY_BUSY_TIMEOUT)?;
            self.backup_before_upgrade(current)?;
            #[cfg(test)]
            trace("backup_done".to_string());
        }
        // 建立迁移事务（BEGIN IMMEDIATE 等写锁）前恢复常驻锁等待值。
        let _tx_busy = crate::retry::apply_busy_timeout(&self.conn, HISTORY_BUSY_TIMEOUT)?;
        #[cfg(test)]
        trace(format!("tx_busy_ms={}", _tx_busy.as_millis()));
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

    /// 升级前备份：`VACUUM INTO` 生成完整的、可直接打开的 SQLite 副本
    ///（等价于 SQLite 在线备份 API），落在历史库同目录。
    ///
    /// 已存在同名备份时不覆盖（升级重试不产生多份），失败即中止升级——
    /// 宁可这次不升级，也不在没有可恢复副本的情况下改结构。
    fn backup_before_upgrade(&self, from_version: i64) -> Result<()> {
        let target = self
            .path
            .with_extension(format!("db.backup-v{from_version}"));
        if target.exists() {
            log::info!("升级前备份已存在，跳过：{}", target.display());
            return Ok(());
        }
        // VACUUM INTO 的路径参数不能用占位符，需内联；单引号按 SQL 规则转义。
        let escaped = target.display().to_string().replace('\'', "''");
        self.conn
            .execute_batch(&format!("VACUUM INTO '{escaped}'"))
            .with_context(|| format!("升级前备份失败（目标 {}）", target.display()))?;
        log::info!(
            "历史库升级前备份完成：v{from_version} → {}",
            target.display()
        );
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

    /// 批次写入（可带一次性标记，**同一事务**提交）。
    ///
    /// 遗留缓存迁移用：全部事件与「已迁移」标记必须原子——中途失败时库内保持
    /// 事务前状态，不留半迁移；标记与数据同时可见，也不会出现"标记已写但数据
    /// 没落库"。
    pub fn write_batch_with_marker(
        &self,
        batch: &[EventWrite],
        marker_key: Option<&str>,
    ) -> Result<WriteSummary> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let generation = self.generation_in_tx(&tx)?;
        let (summary, changed) = write_events_in_tx(&tx, batch, generation)?;
        if let Some(key) = marker_key {
            set_meta_in_tx(&tx, key, "done")?;
        }
        if changed {
            bump_generation(&tx)?;
        }
        tx.commit()?;
        Ok(summary)
    }

    /// 事务内 generation 读取（写事务内校验用：防止"预览后、提交前"的窗口）。
    fn generation_in_tx(&self, tx: &Transaction<'_>) -> Result<u64> {
        let value: Option<String> = tx
            .query_row(
                "SELECT value FROM history_meta WHERE key = ?1",
                params![META_GENERATION],
                |r| r.get(0),
            )
            .optional()?;
        match value {
            Some(v) => parse_u64_text(&v, META_GENERATION),
            None => Ok(0),
        }
    }

    /// 单条写入是否合法（采集侧预检用）：不合法的事件应跳过并计诊断，
    /// **不能**让整批写入因一条坏记录失败（否则一条坏行会让全部采集不回账）。
    pub fn can_write(&self, w: &EventWrite) -> Result<bool> {
        match validate_write(w) {
            Ok(()) => Ok(true),
            Err(e) => {
                log::debug!("写入预检拒绝：{e}");
                Ok(false)
            }
        }
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

    /// 采集专用的升级例外：仅纠正唯一旧原生 Codex 事实的模型归属。
    /// CCS 预演/导入仍调用原写入内核，不进入这条候选匹配路径。
    pub fn write_collected_batch(&self, batch: &[EventWrite]) -> Result<CollectedWriteSummary> {
        let mut result = CollectedWriteSummary::default();
        if batch.is_empty() {
            return Ok(result);
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let generation = self.generation_in_tx(&tx)?;
        let mut changed = false;
        for write in batch {
            validate_write(write)?;
            let mut protected_replay = None;
            let native_codex =
                write.app == AgentKind::Codex && write.precedence == WritePrecedence::NativeLog;
            if native_codex {
                let existing = lookup_event(&tx, write)?;
                match existing.as_ref() {
                    Some(existing)
                        if existing.precedence == WritePrecedence::NativeLog
                            && existing.parser_revision == PARSER_REVISION
                            && existing.model_raw != write.model_raw
                            && same_fact_except_model(existing, write) =>
                    {
                        // 旧别名仍指向纠正后的事实；模型单项重播不得撤销升级纠正。
                        let mut replay = write.clone();
                        replay.model_raw = existing.model_raw.clone();
                        replay.model_identity = existing.model_identity.clone();
                        protected_replay = Some(replay);
                    }
                    _ if write.session_id.as_deref().is_some_and(|id| !id.is_empty()) => {
                        let candidates = old_codex_model_candidates(&tx, write)?;
                        if candidates.len() > 1 {
                            result.ambiguous_models += 1;
                            continue;
                        }
                        if let Some(id) = candidates.first() {
                            if existing.as_ref().is_some_and(|event| event.id != *id) {
                                // 同一行可已持有双别名；只有另一行占用身份才是冲突。
                                bail!("身份冲突：Codex 新模型身份与旧模型候选分别指向不同事实");
                            }
                            // lookup 可先命中主键；纠正不登记新证据，但不能吞掉来源冲突。
                            for origin in &write.origins {
                                if origin.origin_key.is_empty() {
                                    bail!("来源记录键不得为空（{}）", origin.origin_kind);
                                }
                                let owner: Option<i64> = tx
                                    .query_row(
                                        "SELECT event_id FROM event_origins
                                         WHERE origin_kind=?1 AND origin_key=?2",
                                        params![origin.origin_kind, origin.origin_key],
                                        |row| row.get(0),
                                    )
                                    .optional()?;
                                if owner.is_some_and(|owner| owner != *id) {
                                    bail!("来源记录冲突：Codex 模型纠正的来源已指向另一事实");
                                }
                            }
                            tx.execute(
                                "UPDATE usage_events SET model_raw=?2, model_identity=?3,
                                    parser_revision=?4 WHERE id=?1",
                                params![id, write.model_raw, write.model_identity, PARSER_REVISION],
                            )?;
                            // 原行和原证据保持；只追加新模型别名，旧别名不删除。
                            for alias in &write.aliases {
                                if alias.scheme.is_empty() || alias.value.is_empty() {
                                    bail!("别名不得为空（scheme/value）");
                                }
                                let added = tx.execute(
                                    "INSERT INTO event_aliases(event_id, app, identity_scheme,
                                         identity_value, created_generation) VALUES(?1,?2,?3,?4,?5)
                                     ON CONFLICT(app, identity_scheme, identity_value) DO NOTHING",
                                    params![
                                        id,
                                        alias.app.as_str(),
                                        alias.scheme,
                                        alias.value,
                                        generation as i64
                                    ],
                                )?;
                                let owner: i64 = tx.query_row(
                                    "SELECT event_id FROM event_aliases WHERE app=?1
                                         AND identity_scheme=?2 AND identity_value=?3",
                                    params![alias.app.as_str(), alias.scheme, alias.value],
                                    |row| row.get(0),
                                )?;
                                if owner != *id {
                                    bail!("模型纠正别名已绑定其他事件，不能改绑");
                                }
                                result.events.aliases_added += added as u64;
                            }
                            result.models_corrected += 1;
                            result.events.updated += 1;
                            changed = true;
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            let write = protected_replay.as_ref().unwrap_or(write);
            let (summary, wrote) =
                write_events_in_tx(&tx, std::slice::from_ref(write), generation)?;
            // 分项复用原内核，批次累计仍受检；溢出时包含此前纠正在内全部回滚。
            result.events.inserted_tokens = result
                .events
                .inserted_tokens
                .checked_add(&summary.inserted_tokens)
                .ok_or_else(|| anyhow!("本批新增用量累计溢出可表示范围"))?;
            result.events.inserted += summary.inserted;
            result.events.updated += summary.updated;
            result.events.unchanged += summary.unchanged;
            result.events.conflicts += summary.conflicts;
            result.events.stale += summary.stale;
            result.events.overlap_candidates += summary.overlap_candidates;
            result.events.aliases_added += summary.aliases_added;
            result.events.origins_added += summary.origins_added;
            changed |= wrote;
        }
        if changed {
            bump_generation(&tx)?;
        }
        tx.commit()?;
        Ok(result)
    }

    /// 单条写入在库内的处置分类（**预览与提交共用同一判定**，因此预览数字
    /// 与提交结果不会分叉）。只读，不改变库内容。
    pub fn classify_write(&self, w: &EventWrite) -> Result<WriteClass> {
        validate_write(w)?;
        match lookup_event(&self.conn, w)? {
            None => match self.overlap_candidate(w)? {
                // 无身份但有重叠候选：不宣称"就是同一次请求"，按未解决呈现。
                Some(_) if w.aliases.is_empty() => Ok(WriteClass::OverlapCandidate),
                _ => Ok(WriteClass::Insert),
            },
            Some(existing) => Ok(classify_against(&existing, w)),
        }
    }

    /// **批感知**分类（预览用）：按提交顺序逐条判定，判定与提交路径
    /// （[`write_events_in_tx`]）完全同构——
    ///
    /// - 同一事实只有**一份**模拟终值：身份（来源键 / 来源记录 / 别名）只
    ///   映射到事实 ID，经任一身份命中的都是同一份状态；Update 统一前进，
    ///   Unchanged / Conflict / Stale 保持（R01：早期实现按身份各存副本，
    ///   同一事实经不同身份看新旧不一致，预览与提交分叉）；
    /// - **所有**处置类别都登记本条携带的来源记录与别名（提交路径对每一
    ///   类都补记；R01 反例 3：Unchanged 行的新别名必须让后续只携带该别名
    ///   的写入命中同一事实，而不是误报新增）；事件键只在 Insert 时登记
    ///   （行的 event_key 列仅插入时写入，替代键不持久化）；
    /// - 别名在批内或库内已绑定**其他事实**时按提交同口径整批失败；
    /// - `OverlapCandidate` 不记账（是否按新增导入由用户决定）。
    pub fn classify_batch(&self, writes: &[EventWrite]) -> Result<Vec<WriteClass>> {
        let mut sim = BatchSim::new();
        let mut out = Vec::with_capacity(writes.len());
        for w in writes {
            validate_write(w)?;
            let found = sim.resolve(&self.conn, w)?;
            let (class, fact) = match found {
                None => match self.overlap_candidate(w)? {
                    Some(_) if w.aliases.is_empty() => (WriteClass::OverlapCandidate, None),
                    _ => (WriteClass::Insert, None),
                },
                Some(fid) => (classify_against(sim.state(fid), w), Some(fid)),
            };
            match class {
                WriteClass::OverlapCandidate => {}
                WriteClass::Insert => {
                    let fid = sim.insert_fact(w);
                    sim.register(&self.conn, w, fid, true)?;
                }
                _ => {
                    let fid = fact.expect("命中事实必须已登记");
                    if class == WriteClass::Update {
                        sim.apply_update(fid, w);
                    }
                    // 非 Insert 类别不持久化本条的事件键（行列只在插入时写入，
                    // 替代键不得凭空成为后续写入的命中身份——R01 反例 2）。
                    sim.register(&self.conn, w, fid, false)?;
                }
            }
            out.push(class);
        }
        Ok(out)
    }

    /// 重叠候选检测（只读）：库内是否存在同应用、同模型等价身份、四桶相同、
    /// 时间接近的既有请求。仅用于把"身份不足 + 存在重叠"的来源记录标为未解决。
    ///
    /// 时间窗按秒比较（CCS 只到秒，且导入与原生采集的时间戳可能相差数秒到
    /// 数分钟）；窗口内命中即视为候选，不做合并。
    pub fn overlap_candidate(&self, w: &EventWrite) -> Result<Option<StoredEvent>> {
        match lookup_overlap_candidate(&self.conn, w)? {
            Some(id) => load_event(&self.conn, id),
            None => Ok(None),
        }
    }

    /// 日汇总快照在库内的处置分类（只读）。
    ///
    /// 身份 = CCS 完整主键（**不含** `source_tz`——那是导入时记录的统计时区
    /// 假设，不是数据身份；否则同一 CCS 库改选时区再次导入会把同一份汇总
    /// 变成两条、重复累计）。内容比较同样不含时区：仅时区不同 = 同一份
    /// 用量快照，按未变化计（提交时仅修订时区元数据）。
    pub fn classify_rollup(&self, r: &DailyRollupWrite) -> Result<RollupClass> {
        let existing: Option<(i64, String, String, String, String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT request_count, input_tokens, output_tokens, cache_write_tokens,
                        cache_read_tokens, source_cost_usd
                 FROM ccs_daily_usage
                 WHERE logical_source = ?1 AND day = ?2 AND app = ?3
                   AND provider_id = ?4 AND model = ?5 AND request_model = ?6 AND pricing_model = ?7",
                params![
                    r.logical_source,
                    r.day,
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

    /// 导入批次的净新增用量（读审计行；提交时按**本批插入**的事件写入）。
    pub fn import_run_net_tokens(&self, run_id: i64) -> Result<TokenCounts> {
        let (i, o, cw, cr): (String, String, String, String) = self.conn.query_row(
            "SELECT net_input_tokens, net_output_tokens, net_cache_write_tokens,
                    net_cache_read_tokens FROM import_runs WHERE id = ?1",
            params![run_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        Ok(TokenCounts {
            input: parse_u64_text(&i, "net_input_tokens")?,
            output: parse_u64_text(&o, "net_output_tokens")?,
            cache_write: parse_u64_text(&cw, "net_cache_write_tokens")?,
            cache_read: parse_u64_text(&cr, "net_cache_read_tokens")?,
        })
    }

    /// H05：CCS 导入的一个批次——事件、日汇总快照与导入审计**在同一事务**内
    /// 提交（计划 §5.1：批次与数据同事务，崩溃或失败整体回滚）。
    ///
    /// `expected_generation` 在**事务内**校验（不是提交前）：预览之后、提交
    /// 之前发生的任何采集/导入都会让 generation 变化，此时整批拒绝并要求
    /// 重新预览——避免拿过期预览去插入重复请求（计划不变量 12）。
    pub fn apply_import_batch(
        &self,
        writes: &[EventWrite],
        rollups: &[DailyRollupWrite],
        policy: RollupConflictPolicy,
        run: &ImportRunRecord,
        expected_generation: Option<u64>,
    ) -> Result<ImportOutcome> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let generation = self.generation_in_tx(&tx)?;
        if let Some(expected) = expected_generation
            && expected != generation
        {
            bail!(
                "历史库在本次预览之后发生了变化（generation {expected} → {generation}），\
                 请重新预览后再提交"
            );
        }
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
        // 净新增 = **本批插入**事件的四桶之和（只补来源证据的记录不计）。
        let net_new = write_summary.inserted_tokens;
        // 完全相同的重复导入：不新增用量、不重复来源、不递增 generation，
        // 但审计批次照常登记（计划 §3：可以记录审计结果）。
        finish_import_run(&tx, run_id, net_new, &write_summary, &rollup_summary)?;
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

    /// 某个应用的事件数（来源统计用；计数查询，不物化事件）。
    pub fn event_count_of(&self, app: AgentKind) -> Result<u64> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM usage_events WHERE app = ?1",
            params![app.as_str()],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u64)
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

    /// H06：读取日汇总快照（`None` = 全部应用）。
    ///
    /// 返回的是**已去重的来源快照**：CCS 已清理的请求明细在 TokenScope 仍可查，
    /// 而日汇总只提供总量；查询侧据此决定每个 `(日, 应用, 等价模型)` 桶是
    /// 用明细还是用汇总（见 `aggregate::select_rollup_contributions`）。
    pub fn daily_rollups(&self, app: Option<AgentKind>) -> Result<Vec<StoredRollup>> {
        let mut stmt = self.conn.prepare(
            "SELECT logical_source, day, source_tz, app, provider_id, model, request_model,
                    pricing_model, request_count, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens, input_semantics, source_cost_usd
             FROM ccs_daily_usage
             WHERE (?1 IS NULL OR app = ?1)
             ORDER BY day, app, model, provider_id",
        )?;
        let rows = stmt.query_map(params![app.map(|a| a.as_str())], |r| {
            let raw_app: String = r.get(3)?;
            let app = parse_agent(&raw_app).map_err(|e| conversion_error(3, e))?;
            Ok(StoredRollup {
                logical_source: r.get(0)?,
                day: r.get(1)?,
                source_tz: r.get(2)?,
                app,
                provider_id: r.get(4)?,
                model: r.get(5)?,
                request_model: r.get(6)?,
                pricing_model: r.get(7)?,
                request_count: r.get::<_, i64>(8)?.max(0) as u64,
                tokens: TokenCounts {
                    input: u64_from_row(r, 9)?,
                    output: u64_from_row(r, 10)?,
                    cache_write: u64_from_row(r, 11)?,
                    cache_read: u64_from_row(r, 12)?,
                },
                input_semantics: r.get(13)?,
                source_cost_usd: r
                    .get::<_, Option<String>>(14)?
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .filter(|v| v.is_finite()),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
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
/// - **整批原子**：全部写入与一次性标记在**同一个写事务**里提交——中途失败
///   （例如第 501 条写入报错）时库内保持事务前状态，绝不留下"前 500 条已提交"
///   的半迁移；失败不写标记，可用同一份旧库原样重试。
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
        // N03：读取阶段拒绝的负数字段行计入诊断（负值不伪装成合法零值）。
        report.rejected += file.rejected;
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
    // 整批一个事务 + 标记同事务提交：失败整体回滚（计划 §6 的事务前状态）。
    let summary = history.write_batch_with_marker(&batch, Some(META_LEGACY_MIGRATION))?;
    report.inserted = summary.inserted;
    report.updated = summary.updated;
    report.unchanged = summary.unchanged;
    report.conflicts = summary.conflicts;
    report.stale = summary.stale;
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
    // 桶值已在读取期受检为非负（N03）：四桶合计通过即蕴含 prompt 基数
    //（非缓存输入 + 缓存写 + 缓存读）不溢出，无需重复校验。
    // 旧缓存里的行序（v11+）优先；更早版本没有该列，用行位置兜底。
    let position = if ev.line > 0 { ev.line } else { idx as u64 };
    // 身份别名：迁移过来的行必须能与"迁移后重扫"命中同一事件，否则同一请求
    // 会被计两次（Claude 用可证实身份，Codex 用与全局去重一致的保守重播身份）。
    let identity_event = crate::model::UsageEvent {
        ts,
        agent: file.app,
        model: ev.model.clone(),
        session_id: ev.session_id.clone(),
        project: ev.project.clone(),
        session_initial_cwd: ev.session_initial_cwd.clone(),
        event_cwd: ev.event_cwd.clone(),
        record_id: ev.record_id.clone(),
        line: 0,
        source_path: String::new(),
        input_tokens: ev.input,
        output_tokens: ev.output,
        cache_write_tokens: ev.cache_write,
        cache_read_tokens: ev.cache_read,
    };
    let mut aliases = Vec::new();
    if let Some((scheme, value)) = identity_event.same_source_identity() {
        aliases.push(EventAlias {
            app: file.app,
            scheme: scheme.to_string(),
            value,
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
                // 身份不足（无别名）且库内有重叠候选时记录审计计数：调用方
                // 可能在用户确认后按"新增"导入，这条计数让结果可核查。
                if write.aliases.is_empty() && lookup_overlap_candidate(tx, write)?.is_some() {
                    summary.overlap_candidates += 1;
                }
                let id = insert_event(tx, write)?;
                summary.inserted += 1;
                summary.inserted_tokens = summary
                    .inserted_tokens
                    .checked_add(&write.tokens)
                    .ok_or_else(|| anyhow!("本批新增用量累计溢出可表示范围"))?;
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

/// 重叠候选查询（`&Connection` 版本：写事务内外共用同一判定）。
fn lookup_overlap_candidate(conn: &Connection, w: &EventWrite) -> Result<Option<i64>> {
    const WINDOW_SECONDS: i64 = 600;
    Ok(conn
        .query_row(
            "SELECT id FROM usage_events
             WHERE app = ?1 AND model_identity = ?2
               AND input_tokens = ?3 AND output_tokens = ?4
               AND cache_write_tokens = ?5 AND cache_read_tokens = ?6
               AND ts_seconds BETWEEN ?7 AND ?8
             ORDER BY ABS(ts_seconds - ?9), id
             LIMIT 1",
            params![
                w.app.as_str(),
                w.model_identity,
                w.tokens.input.to_string(),
                w.tokens.output.to_string(),
                w.tokens.cache_write.to_string(),
                w.tokens.cache_read.to_string(),
                w.ts.as_second() - WINDOW_SECONDS,
                w.ts.as_second() + WINDOW_SECONDS,
                w.ts.as_second(),
            ],
            |r| r.get::<_, i64>(0),
        )
        .optional()?)
}

/// 库内处置判定（纯函数，预览与写入共用）。
///
/// 优先级**先于**观察时间：原生日志比 CCS 导入更可信，同一条请求的原生记录
/// 无论事件时间是否早于已存 CCS 值都要胜出（计划 §5.3：原生记录按原生终值
/// 规则选取，与导入顺序无关）——否则 CCS 的 `created_at` 稍晚几秒时，后采集
/// 的原生终值会被"观察时间更早"误判为过期。观察时间只在**同优先级**内区分
/// 新旧（同请求的流式更新、旧副本回放）。
fn classify_against(existing: &StoredEvent, w: &EventWrite) -> WriteClass {
    if content_equal(existing, w) {
        if w.precedence > existing.precedence {
            // P2-2：内容等值但更高优先级来源（原生日志）确认了同一事实——
            // 仅升级 origin_rank（值不变）。否则先 CCS 后原生的确认停留在
            // 低优先级，后续同请求的 CCS 变化会被静默覆盖而非按冲突保留
            //（merge_event 的 Update 以等值内容重写同一行，幂等且只推进
            // rank；BatchSim.apply_update 同步推进模拟状态）。
            WriteClass::Update
        } else {
            WriteClass::Unchanged
        }
    } else if w.precedence > existing.precedence {
        WriteClass::Update
    } else if existing.precedence > w.precedence {
        WriteClass::Conflict
    } else if w.observed_at < existing.observed_at {
        WriteClass::Stale
    } else {
        WriteClass::Update
    }
}

/// 库内同键日汇总快照的读取形态（含来源时区与批次归属，供诊断）。
type RollupSnapshotRow = (
    i64,
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<i64>,
);

/// 日汇总快照写入：同键内容相同 → 跳过（仅来源时区假设不同时修订该元数据，
/// 用量未变仍计未变化）；内容不同 → 按策略保留或替换（来源无可验证修订时
/// 默认保留已存值并计冲突）。同键身份 = CCS 完整主键，**不含** `source_tz`。
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
                        cache_read_tokens, source_cost_usd, source_tz, import_run_id
                 FROM ccs_daily_usage
                 WHERE logical_source = ?1 AND day = ?2 AND app = ?3
                   AND provider_id = ?4 AND model = ?5 AND request_model = ?6 AND pricing_model = ?7",
                params![
                    r.logical_source,
                    r.day,
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
                        row.get(8)?,
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
            Some((id, count, i, o, cw, cr, cost, existing_tz, _run)) => {
                let same = count == r.request_count as i64
                    && i == r.tokens.input.to_string()
                    && o == r.tokens.output.to_string()
                    && cw == r.tokens.cache_write.to_string()
                    && cr == r.tokens.cache_read.to_string()
                    && cost == r.source_cost_usd.map(|v| v.to_string());
                if same {
                    if existing_tz != r.source_tz {
                        // 同一份快照、来源时区假设变了：修订元数据（查询侧的
                        // 日时区不匹配提示据此更准），用量与请求数不动。
                        tx.execute(
                            "UPDATE ccs_daily_usage SET source_tz = ?2 WHERE id = ?1",
                            params![id, r.source_tz],
                        )?;
                        changed = true;
                    }
                    summary.unchanged += 1;
                } else if r.revision > 0 || policy == RollupConflictPolicy::TakeSource {
                    tx.execute(
                        "UPDATE ccs_daily_usage SET request_count = ?2, input_tokens = ?3,
                            output_tokens = ?4, cache_write_tokens = ?5, cache_read_tokens = ?6,
                            input_semantics = ?7, source_cost_usd = ?8, revision = ?9,
                            source_tz = ?10
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
                            r.source_tz,
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
    net_new: TokenCounts,
    write_summary: &WriteSummary,
    rollup_summary: &RollupSummary,
) -> Result<()> {
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
            net_new.input.to_string(),
            net_new.output.to_string(),
            net_new.cache_write.to_string(),
            net_new.cache_read.to_string(),
        ],
    )?;
    Ok(())
}

/// 事务内写入元信息（一次性标记等）。
fn set_meta_in_tx(tx: &Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO history_meta(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
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

fn same_fact_except_model(existing: &StoredEvent, write: &EventWrite) -> bool {
    existing.app == write.app
        && existing.ts == write.ts
        && existing.session_id == write.session_id
        && existing.record_id == write.record_id
        && existing.project_key == write.project_key
        && existing.session_initial_cwd == write.session_initial_cwd
        && existing.event_cwd == write.event_cwd
        && existing.tokens == write.tokens
}

/// 精确时间索引限定候选，再检查四桶与其余事实字段；路径/行号不参与。
/// 至多取两个即可区分全库唯一与歧义，不用模糊重叠窗口或猜测首条。
fn old_codex_model_candidates(conn: &Connection, write: &EventWrite) -> Result<Vec<i64>> {
    let mut statement = conn.prepare(
        "SELECT id FROM usage_events WHERE app='codex' AND origin_rank=?1
             AND parser_revision<?2 AND session_id=?3 AND ts_seconds=?4 AND ts_nanos=?5
             AND model_raw<>?6 AND input_tokens=?7 AND output_tokens=?8
             AND cache_write_tokens=?9 AND cache_read_tokens=?10
             AND record_id IS ?11 AND project_key IS ?12
             AND session_initial_cwd IS ?13 AND event_cwd IS ?14 LIMIT 2",
    )?;
    let rows = statement.query_map(
        params![
            WritePrecedence::NativeLog.rank(),
            PARSER_REVISION,
            write.session_id,
            write.ts.as_second(),
            write.ts.subsec_nanosecond() as i64,
            write.model_raw,
            write.tokens.input.to_string(),
            write.tokens.output.to_string(),
            write.tokens.cache_write.to_string(),
            write.tokens.cache_read.to_string(),
            write.record_id,
            write.project_key,
            write.session_initial_cwd,
            write.event_cwd
        ],
        |row| row.get(0),
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 批内模拟状态（[`HistoryDb::classify_batch`] 用）：身份 → 事实 → 统一终值。
struct BatchSim {
    /// 身份键（`k|来源键`、`o|来源记录键`、`a|app|scheme|value`）→ 事实 ID。
    claims: std::collections::HashMap<String, i64>,
    /// 库内事件 ID → 事实 ID（同一库内事件在批内只对应一个事实）。
    resolved: std::collections::HashMap<i64, i64>,
    /// 事实 ID（负数，与库内正数主键不重叠）→ 统一模拟终值。
    facts: std::collections::HashMap<i64, StoredEvent>,
    next_fact: i64,
}

fn alias_claim_key(a: &EventAlias) -> String {
    format!("a|{}|{}|{}", a.app.as_str(), a.scheme, a.value)
}

/// 来源记录的批内身份键：**完整二元组** `(origin_kind, origin_key)`——与
/// schema 的 UNIQUE 约束和真实 lookup/提交同域（T01：仅用 key 会让不同
/// 种类的同值键被当成同一身份）。
fn origin_claim_key(o: &EventOrigin) -> String {
    format!("o|{}|{}", o.origin_kind, o.origin_key)
}

/// 写入的模拟行形态（`id` = 事实 ID；`event_key` 在 Insert 时取写入键）。
fn sim_event(w: &EventWrite, fid: i64) -> StoredEvent {
    StoredEvent {
        id: fid,
        event_key: w.event_key.clone(),
        app: w.app,
        ts: w.ts,
        model_raw: w.model_raw.clone(),
        model_identity: w.model_identity.clone(),
        identity_revision: IDENTITY_REVISION,
        session_id: w.session_id.clone(),
        record_id: w.record_id.clone(),
        project_key: w.project_key.clone(),
        session_initial_cwd: w.session_initial_cwd.clone(),
        event_cwd: w.event_cwd.clone(),
        tokens: w.tokens,
        precedence: w.precedence,
        observed_at: w.observed_at,
        parser_revision: w.parser_revision(),
    }
}

impl BatchSim {
    fn new() -> Self {
        Self {
            claims: std::collections::HashMap::new(),
            resolved: std::collections::HashMap::new(),
            facts: std::collections::HashMap::new(),
            next_fact: -1,
        }
    }

    fn state(&self, fid: i64) -> &StoredEvent {
        self.facts
            .get(&fid)
            .expect("事实必须已登记（resolve/insert 之后）")
    }

    /// 库内事件 → 事实（首次遇到时载入为该事实的初始终值）。
    fn fact_of(&mut self, conn: &Connection, event_id: i64) -> Result<i64> {
        if let Some(f) = self.resolved.get(&event_id) {
            return Ok(*f);
        }
        let ev = load_event(conn, event_id)?.ok_or_else(|| anyhow!("事件 {event_id} 不存在"))?;
        let fid = self.next_fact;
        self.next_fact -= 1;
        self.resolved.insert(event_id, fid);
        self.facts.insert(fid, ev);
        Ok(fid)
    }

    /// 解析本条写入命中的事实：与 [`lookup_event`] 的三级顺序与冲突语义
    /// 一致（来源键 → 来源记录 → 别名；本批声明优先于库内行，按事实归一）。
    fn resolve(&mut self, conn: &Connection, w: &EventWrite) -> Result<Option<i64>> {
        // 1) 来源键。
        if let Some(f) = self.claims.get(&format!("k|{}", w.event_key)) {
            return Ok(Some(*f));
        }
        let by_key: Option<i64> = conn
            .query_row(
                "SELECT id FROM usage_events WHERE event_key = ?1",
                params![w.event_key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = by_key {
            return self.fact_of(conn, id).map(Some);
        }
        // 2) 来源记录证据。
        for origin in &w.origins {
            if let Some(f) = self.claims.get(&origin_claim_key(origin)) {
                return Ok(Some(*f));
            }
            let hit: Option<i64> = conn
                .query_row(
                    "SELECT event_id FROM event_origins WHERE origin_kind = ?1 AND origin_key = ?2",
                    params![origin.origin_kind, origin.origin_key],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = hit {
                return self.fact_of(conn, id).map(Some);
            }
        }
        // 3) 身份别名：可多命中，按事实去重；指向多个事实 → 与提交同口径冲突。
        let mut hits: Vec<i64> = Vec::new();
        for alias in &w.aliases {
            let key = alias_claim_key(alias);
            if let Some(f) = self.claims.get(&key) {
                if !hits.contains(f) {
                    hits.push(*f);
                }
                continue;
            }
            let hit: Option<i64> = conn
                .query_row(
                    "SELECT event_id FROM event_aliases
                     WHERE app = ?1 AND identity_scheme = ?2 AND identity_value = ?3",
                    params![alias.app.as_str(), alias.scheme, alias.value],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = hit {
                let f = self.fact_of(conn, id)?;
                if !hits.contains(&f) {
                    hits.push(f);
                }
            }
        }
        match hits.len() {
            0 => Ok(None),
            1 => Ok(Some(hits[0])),
            _ => bail!("身份冲突：同一请求的多个别名分别指向不同事实，无法在不猜测的前提下合并"),
        }
    }

    /// 新建事实（Insert）：模拟行 = 本条写入内容。
    fn insert_fact(&mut self, w: &EventWrite) -> i64 {
        let fid = self.next_fact;
        self.next_fact -= 1;
        self.facts.insert(fid, sim_event(w, fid));
        fid
    }

    /// Update：统一终值前进为本次写入（事件键保持事实原值——提交路径的
    /// UPDATE 不改 event_key）。
    fn apply_update(&mut self, fid: i64, w: &EventWrite) {
        if let Some(f) = self.facts.get_mut(&fid) {
            let key = f.event_key.clone();
            *f = sim_event(w, fid);
            f.event_key = key;
        }
    }

    /// 证据登记：与 [`write_events_in_tx`] 同口径——
    ///
    /// - **所有**处置类别都登记本条携带的来源记录与别名（提交对每一类都
    ///   补记）；别名/来源记录已绑定**其他事实**时按提交同口径报错（提交
    ///   会整批失败，预览不得假装可行——R01 反例 1：来源记录缺冲突校验）；
    /// - 事件键**只在 Insert 时**登记（行的 event_key 列仅插入时写入；
    ///   Unchanged/Update/Conflict/Stale 不持久化本条的替代键——R01 反例 2：
    ///   凭空登记会让后续同键写入错误命中旧事实）。
    fn register(
        &mut self,
        conn: &Connection,
        w: &EventWrite,
        fid: i64,
        persist_key: bool,
    ) -> Result<()> {
        if persist_key {
            self.claims.insert(format!("k|{}", w.event_key), fid);
        }
        for o in &w.origins {
            let okey = origin_claim_key(o);
            if let Some(other) = self.claims.get(&okey) {
                if *other != fid {
                    bail!("来源记录冲突：{okey} 已绑定其他事实，提交将整批失败");
                }
                continue;
            }
            let bound: Option<i64> = conn
                .query_row(
                    "SELECT event_id FROM event_origins WHERE origin_kind = ?1 AND origin_key = ?2",
                    params![o.origin_kind, o.origin_key],
                    |r| r.get(0),
                )
                .optional()?;
            match bound {
                Some(id) => {
                    let other = self.fact_of(conn, id)?;
                    if other != fid {
                        bail!("来源记录冲突：{okey} 已绑定其他事实，提交将整批失败");
                    }
                    // 已绑定同一事实：与提交路径的 no-op 一致。
                }
                None => {
                    self.claims.insert(okey, fid);
                }
            }
        }
        for a in &w.aliases {
            let key = alias_claim_key(a);
            if let Some(other) = self.claims.get(&key) {
                if *other != fid {
                    bail!("别名冲突：{key} 已绑定其他事实，提交将整批失败");
                }
                continue;
            }
            let bound: Option<i64> = conn
                .query_row(
                    "SELECT event_id FROM event_aliases
                     WHERE app = ?1 AND identity_scheme = ?2 AND identity_value = ?3",
                    params![a.app.as_str(), a.scheme, a.value],
                    |r| r.get(0),
                )
                .optional()?;
            match bound {
                Some(id) => {
                    let other = self.fact_of(conn, id)?;
                    if other != fid {
                        bail!("别名冲突：{key} 已绑定其他事实，提交将整批失败");
                    }
                    // 已绑定同一事实：与提交路径的 no-op 一致。
                }
                None => {
                    self.claims.insert(key, fid);
                }
            }
        }
        Ok(())
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
        WriteClass::Insert | WriteClass::Update | WriteClass::OverlapCandidate => {}
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
    /// 事件解析版本：恒为 [`PARSER_REVISION`]（`EventWrite` 没有可覆盖的
    /// `parser_revision` 字段——持久化列由适配器在构造时决定）。
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

    fn collected_model_write(key: &str, model: &str) -> EventWrite {
        EventWrite {
            event_key: key.into(),
            app: AgentKind::Codex,
            ts: "2026-10-22T08:00:00.123456789Z".parse().unwrap(),
            model_raw: model.into(),
            model_identity: crate::model_identity::ModelIdentity::parse(model).identity_key(),
            session_id: Some("synthetic-collection-model".into()),
            record_id: None,
            project_key: None,
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 80,
                output: 10,
                cache_write: 0,
                cache_read: 20,
            },
            precedence: WritePrecedence::NativeLog,
            observed_at: "2026-10-22T08:00:00.123456789Z".parse().unwrap(),
            aliases: vec![EventAlias {
                app: AgentKind::Codex,
                scheme: "codex-usage".into(),
                value: format!("synthetic-collection-model|{model}|80|10|0|20"),
            }],
            origins: vec![EventOrigin {
                origin_kind: ORIGIN_KIND_NATIVE_FILE.into(),
                origin_key: key.into(),
                app: AgentKind::Codex,
                import_run_id: None,
                parser_revision: 1,
                source_model_raw: None,
                pricing_model: None,
                source_cost_usd: None,
                ts_precision_seconds: false,
            }],
        }
    }

    #[test]
    fn protected_model_replay_still_rejects_identity_conflicts_and_rolls_back_batch() {
        for conflict_is_alias in [true, false] {
            let db = HistoryDb::open(Path::new(":memory:")).unwrap();
            let old = collected_model_write("old", "model-a");
            let unrelated = collected_model_write("other", "model-c");
            db.write_batch(&[old.clone(), unrelated.clone()]).unwrap();
            db.conn
                .execute(
                    "UPDATE usage_events SET parser_revision=1 WHERE event_key='old'",
                    [],
                )
                .unwrap();
            let correction = db
                .write_collected_batch(&[collected_model_write("new", "model-b")])
                .unwrap();
            assert_eq!(correction.models_corrected, 1);
            let before = db.stored_events().unwrap();
            let generation = db.generation().unwrap();
            let mut replay = old;
            if conflict_is_alias {
                replay.aliases.push(unrelated.aliases[0].clone());
            } else {
                replay.origins.push(unrelated.origins[0].clone());
            }
            let inserted = collected_model_write("insert-before-conflict", "model-d");
            let result = db.write_collected_batch(&[inserted, replay]);
            assert!(result.is_err(), "保护旧模型回放仍必须校验身份冲突");
            assert_eq!(db.stored_events().unwrap(), before);
            assert_eq!(db.generation().unwrap(), generation);
        }
    }

    #[test]
    fn model_correction_rejects_conflicting_origin_and_rolls_back_prior_writes() {
        let db = HistoryDb::open(Path::new(":memory:")).unwrap();
        let old = collected_model_write("old", "model-a");
        let unrelated = collected_model_write("other", "model-c");
        db.write_batch(&[old, unrelated.clone()]).unwrap();
        db.conn
            .execute(
                "UPDATE usage_events SET parser_revision=1 WHERE event_key='old'",
                [],
            )
            .unwrap();
        let before = db.stored_events().unwrap();
        let generation = db.generation().unwrap();
        let mut correction = collected_model_write("old", "model-b");
        correction.origins.push(unrelated.origins[0].clone());
        let mut inserted = collected_model_write("insert-before-conflict", "model-d");
        inserted.ts = "2026-10-22T08:00:01.123456789Z".parse().unwrap();
        inserted.observed_at = inserted.ts;
        let result = db.write_collected_batch(&[inserted, correction]);
        assert!(result.is_err(), "模型纠正不能绕过已绑定另一事实的来源冲突");
        assert_eq!(db.stored_events().unwrap(), before);
        assert_eq!(db.generation().unwrap(), generation);
        let aliases: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM event_aliases", [], |r| r.get(0))
            .unwrap();
        assert_eq!(aliases, 2, "不得残留纠正或前条插入的别名");
    }

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

    /// 迁移失败的包装必须同时满足两个约束：单层 Display 带出原因（不被
    /// 位置说明压成无信息量的话），source 链保留类型化错误（打开层重试按
    /// 类型判定瞬态错误）。
    #[test]
    fn migrate_failure_keeps_reason_in_display_and_type_in_source() {
        let typed = anyhow::Error::new(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ReadOnly,
                extended_code: 8,
            },
            Some("attempt to write a readonly database".into()),
        ));
        let wrapped = anyhow::Error::new(MigrateFailure {
            detail: format!("{:#}（历史库文件保留在 X，可重试；不执行删表重建）", typed),
            source: typed,
        });
        let text = wrapped.to_string();
        assert!(
            text.contains("attempt to write a readonly database"),
            "Display 必须带出原因：{text}"
        );
        assert!(
            text.contains("历史库文件保留在 X"),
            "Display 必须带出保留位置：{text}"
        );
        assert!(
            crate::retry::is_concurrent_init_transient(&wrapped),
            "source 链必须保留类型化错误，供打开层按类型重试"
        );
    }

    /// 旧库确实需要迁移：持锁失败保留类型，等待按固定 5 秒而非重试余量。
    /// 上界允许备份和迁移两个等待阶段及调度容差，不声称 I/O 有硬超时。
    #[test]
    fn migration_lock_wait_uses_resident_timeout() {
        let dir = std::env::temp_dir().join(format!(
            "tokenscope-lockwait-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.db");
        seed_v3_history(&path);
        let holder = Connection::open(&path).unwrap();
        // 持有写锁：另一个 open 的 WAL 切换与迁移等待都会受阻。
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let started = std::time::Instant::now();
        let err = HistoryDb::open(&path).unwrap_err();
        let elapsed = started.elapsed();
        holder.execute_batch("ROLLBACK").unwrap();
        // ROLLBACK 只释放事务锁，连接仍持有文件句柄：先关闭连接再删，
        // 否则 Windows 上删除失败（计划 TL03：先释放本用例自己的资源）。
        drop(holder);
        // 收尾必须核实：删除失败（如连接仍持有文件句柄）要让本用例红，
        // 不能用忽略错误把「测试通过但目录残留」掩盖掉（计划 TL03）。
        let cleanup = std::fs::remove_dir_all(&dir);
        assert!(
            cleanup.is_ok(),
            "夹具清理失败 {}: {cleanup:?}",
            dir.display()
        );
        assert!(!dir.exists(), "夹具目录仍存在: {}", dir.display());

        assert!(
            crate::retry::is_concurrent_init_transient(&err),
            "持锁失败必须是类型化的瞬态错误：{err:#}"
        );
        let budget = std::time::Duration::from_millis(5000);
        assert!(
            elapsed >= budget.saturating_sub(std::time::Duration::from_millis(500)),
            "锁等待应接近预算（允许调度容差）：{elapsed:?}"
        );
        assert!(
            elapsed <= budget * 2 + std::time::Duration::from_secs(2),
            "锁等待不得远超预算（OS I/O 之外的有界等待）：{elapsed:?}"
        );
    }

    /// 并发首次初始化：两个连接同时对**新建**库执行 open（WAL 切换 + 迁移）。
    /// 修复前实测（2026-10-19 探针，30 轮）：11 轮有一方在 8-ddl（9 次）或
    /// 3-wal（2 次）阶段报 SQLITE_READONLY（extended=8）；预建库后并发打开、
    /// 保持连接存活的并发打开、串行打开三组对照均 30/30 通过。回归保持原
    /// 并发强度（两线程同时初始化），不得用重试或串行化掩盖。
    #[test]
    fn concurrent_first_initialization_both_succeed() {
        for round in 0..50 {
            let dir = std::env::temp_dir().join(format!(
                "tokenscope-first-init-{}-{round}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("history.db");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    let p = path.clone();
                    let b = barrier.clone();
                    std::thread::spawn(move || {
                        b.wait();
                        HistoryDb::open(&p)
                    })
                })
                .collect();
            for h in handles {
                match h.join().unwrap() {
                    Ok(_db) => {}
                    Err(e) => panic!("第 {round} 轮并发首次初始化必须双方成功：{e:#}"),
                }
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// 每次 `now()` 推进固定步长的假时钟：生产代码在预算检查点读时钟，
    /// 因此可模拟「两个检查点之间发生了真实 I/O（升级前备份）消耗」，
    /// 让测试覆盖 `open → open_once → migrate` 的**生产接线**。
    struct StepClock {
        now: std::cell::Cell<std::time::Instant>,
        step: std::time::Duration,
    }

    impl crate::retry::Clock for StepClock {
        fn now(&self) -> std::time::Instant {
            let t = self.now.get();
            self.now.set(t + self.step);
            t
        }

        fn sleep(&self, d: std::time::Duration) {
            self.now.set(self.now.get() + d);
        }
    }

    /// 构造真实 v3 库（完整 v1–v3 结构）：升级到 v4 会先做备份并建立迁移
    /// 事务，覆盖 migrate 的备份与事务路径。
    fn seed_v3_history(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        for (version, sql) in MIGRATIONS {
            if *version <= 3 {
                conn.execute_batch(sql).unwrap();
            }
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
    }

    fn unique_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tokenscope-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn opened_history_keeps_resident_timeout() {
        let dir = unique_temp_dir("resident-timeout");
        let db = HistoryDb::open(&dir.join("history.db")).unwrap();
        let timeout: i64 = db
            .conn
            .pragma_query_value(None, "busy_timeout", |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn slow_successful_history_open_outlives_retry_deadline() {
        let dir = unique_temp_dir("slow-open");
        let clock = StepClock {
            now: std::cell::Cell::new(std::time::Instant::now()),
            step: std::time::Duration::from_secs(3),
        };
        let db = HistoryDb::open_with_clock(&dir.join("history.db"), &clock).unwrap();
        let timeout: i64 = db
            .conn
            .pragma_query_value(None, "busy_timeout", |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn latest_history_opens_without_writer_lock() {
        let dir = unique_temp_dir("latest-no-write");
        let path = dir.join("history.db");
        let holder = HistoryDb::open(&path).unwrap();
        holder.set_meta(META_GENERATION, "42").unwrap();
        holder.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        let start = std::time::Instant::now();
        let opened = HistoryDb::open(&path).and_then(|db| {
            let generation = db.read_snapshot()?.generation()?;
            Ok((db, generation))
        });
        let elapsed = start.elapsed();
        // 先解除锁，再断言，失败也不遗留持锁连接。
        holder.conn.execute_batch("ROLLBACK").unwrap();
        let (db, generation) = opened.unwrap();
        assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
        assert_eq!(generation, 42);
        drop(db);
        drop(holder);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn latest_history_repairs_each_missing_meta_without_overwrite() {
        for missing in [
            META_GENERATION,
            META_PARSER_REVISION,
            META_IDENTITY_REVISION,
            META_CREATED_UTC,
        ] {
            let dir = unique_temp_dir("missing-meta");
            let path = dir.join("history.db");
            let db = HistoryDb::open(&path).unwrap();
            for key in [
                META_GENERATION,
                META_PARSER_REVISION,
                META_IDENTITY_REVISION,
                META_CREATED_UTC,
            ] {
                db.set_meta(key, "42").unwrap();
            }
            db.conn
                .execute("DELETE FROM history_meta WHERE key = ?1", [missing])
                .unwrap();
            drop(db);
            let db = HistoryDb::open(&path).unwrap();
            for key in [
                META_GENERATION,
                META_PARSER_REVISION,
                META_IDENTITY_REVISION,
                META_CREATED_UTC,
            ] {
                let value = db.meta(key).unwrap().expect("缺失键必须补齐");
                if key != missing {
                    assert_eq!(value, "42", "不得覆盖既有键 {key}");
                }
            }
            drop(db);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    /// 备份即使超过重试截止时间，成功路径仍应完成迁移。
    #[test]
    fn backup_outliving_retry_deadline_still_migrates() {
        let _ = take_trace();
        let dir = unique_temp_dir("backup-budget");
        let path = dir.join("history.db");
        seed_v3_history(&path);
        // 仅在真实备份文件生成后跨过截止时间，避免在 WAL 阶段就提前失败。
        struct BackupClock {
            start: std::time::Instant,
            backup: PathBuf,
        }
        impl crate::retry::Clock for BackupClock {
            fn now(&self) -> std::time::Instant {
                if self.backup.exists() {
                    self.start + std::time::Duration::from_secs(3)
                } else {
                    self.start
                }
            }
            fn sleep(&self, _: std::time::Duration) {
                panic!("无竞争的成功备份路径不得重试");
            }
        }
        let clock = BackupClock {
            start: std::time::Instant::now(),
            backup: path.with_extension("db.backup-v3"),
        };
        let db = HistoryDb::open_with_clock(&path, &clock).unwrap();
        let trace = take_trace();
        assert!(trace.iter().any(|t| t == "backup_done"), "{trace:?}");
        assert!(
            crate::retry::Clock::now(&clock)
                >= clock.start + crate::retry::CONCURRENT_INIT_RETRY_BUDGET,
            "备份完成后时钟必须已越过重试截止时间"
        );
        assert!(trace.iter().any(|t| t == "tx_busy_ms=5000"), "{trace:?}");
        assert!(path.with_extension("db.backup-v3").exists());
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            HISTORY_SCHEMA_VERSION,
            "成功备份后应完成升级"
        );
        drop(conn);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 打开流程推进时钟（步进假时钟）时，迁移事务仍使用固定 5000ms 锁等待值。
    #[test]
    fn migration_transaction_keeps_resident_timeout_after_backup() {
        let _ = take_trace();
        let dir = unique_temp_dir("backup-partial-budget");
        let path = dir.join("history.db");
        seed_v3_history(&path);
        let clock = StepClock {
            now: std::cell::Cell::new(std::time::Instant::now()),
            step: std::time::Duration::from_millis(100),
        };
        let db = HistoryDb::open_with_clock(&path, &clock).expect("部分消耗预算内应完成升级");
        let trace = take_trace();
        let value = |prefix: &str| -> Option<u128> {
            trace
                .iter()
                .find_map(|t| t.strip_prefix(prefix)?.trim().parse().ok())
        };
        assert!(
            trace.iter().any(|t| t == "backup_done"),
            "必须经过备份：{trace:?}"
        );
        let tx_busy = value("tx_busy_ms=").expect("必须建立迁移事务");
        assert_eq!(tx_busy, 5000, "事务使用固定常驻等待值");
        assert_eq!(
            db.schema_version().unwrap(),
            HISTORY_SCHEMA_VERSION,
            "升级应完成"
        );
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
