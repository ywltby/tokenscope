//! H05：从 cc-switch 导入用量（计划 §5）。只读来源库，写入 TokenScope 自己的历史库。
//!
//! 与 cc-switch 的口径对齐（已按 `main` 分支源码与本机库结构核实）：
//!
//! - **输入归一化**：`input_token_semantics = 0`（legacy，input 含缓存读）、
//!   `1`（total-inclusive，input 含缓存读与写）、`2`（fresh，input 已是非缓存
//!   输入）。**cache-inclusive 应用**（codex / gemini / grokbuild）按语义扣减；
//!   Claude 的 input 本身不含缓存，不扣。等价于 CCS 的 `fresh_input_sql`
//!   （`src-tauri/src/services/sql_helpers.rs`）。
//! - **有效用量过滤**：会话日志来源的行（`session_log` / `codex_session` /
//!   `gemini_session` / `opencode_session`）若在同一应用下有指纹相同、模型相同、
//!   时间窗 ±10 分钟的成功 proxy 行，则视为同一次请求的副本，不重复导入
//!   （等价于 CCS 的 `effective_usage_log_filter`）。不做这一步会把这批行当成
//!   独立请求重复计费。
//! - **时间**：`created_at` 是 Unix 秒；事件时间按秒级精度保存，不伪造纳秒。
//! - **身份**：Claude 会话行的 `request_id` 形如 `session:<message.id>`，与
//!   `session_id` 合起来正是 TokenScope 原生采集所用的
//!   `claude-message = <session_id>|<message.id>` 身份，因此两侧可绑定到同一条
//!   历史事件；Codex 的 `token_count` 不带请求标识，导入侧登记与原生采集同构
//!   的保守重播身份（`codex-usage = session|模型|四桶`）——缺 session_id 时不
//!   猜测身份，只按来源记录键去重，库内有同时间同用量候选时列为重叠候选。
//! - **日汇总**：CCS 会把旧明细归并成日粒度并删除明细，导入必须同时覆盖两张表；
//!   日汇总单独保存（`ccs_daily_usage`），不伪造成请求事件、不与明细相加。
//! - **缺 cwd**：请求明细没有 cwd/project 列，因此项目保持未知，不按模型、
//!   日期或同步文件路径猜分配。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::history::{
    DailyRollupWrite, EventAlias, EventOrigin, EventWrite, HistoryDb, ImportRunRecord,
    ORIGIN_KIND_CCS_REQUEST, RollupClass, RollupConflictPolicy, WriteClass, WritePrecedence,
};
use crate::model::{AgentKind, TokenCounts};

/// 逻辑来源标识：本期只支持一个 CCS 逻辑来源——文件路径与导入时刻不充当用量
/// 身份，换成同一库的备份再导入仍更新同一逻辑来源（计划 §5.3）。
pub const CCS_LOGICAL_SOURCE: &str = "ccs";

/// CCS `input_token_semantics` 取值。
pub const INPUT_TOKEN_SEMANTICS_LEGACY: i64 = 0;
pub const INPUT_TOKEN_SEMANTICS_TOTAL: i64 = 1;
pub const INPUT_TOKEN_SEMANTICS_FRESH: i64 = 2;

/// `input_tokens` 已包含缓存读/写的应用（与 CCS 的 `CACHE_INCLUSIVE_APP_TYPES`
/// 同口径）。新增应用时必须同步这张表，否则输入会被重复计入。
const CACHE_INCLUSIVE_APP_TYPES: &[&str] = &["codex", "gemini", "grokbuild"];

/// 会话日志来源的 `data_source` 取值。
const SESSION_DATA_SOURCES: &[&str] = &[
    "session_log",
    "codex_session",
    "gemini_session",
    "opencode_session",
];

/// 会话行与 proxy 行的去重时间窗（秒），与 CCS 同值。
const SESSION_PROXY_DEDUP_WINDOW_SECONDS: i64 = 600;

/// 会话导入的 `request_id` 前缀。
const SESSION_REQUEST_ID_PREFIX: &str = "session:";

/// 预览计划有效期：过期需重新预览（计划 §5.1）。
const PLAN_TTL: Duration = Duration::from_secs(600);

/// 单次预览可暂存的明细上限（超出即拒绝并要求分批；本机量级远低于此）。
const MAX_STAGED_REQUESTS: usize = 500_000;

/// 默认来源库路径（`~/.cc-switch/cc-switch.db`）。
pub fn default_source_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
    Ok(home.join(".cc-switch").join("cc-switch.db"))
}

/// 来源库路径是否存在（仅用于界面提示，不打开库）。
pub fn source_exists(path: &Path) -> bool {
    path.is_file()
}

/// 只读打开的来源库连接。
pub struct CcsSource {
    conn: Connection,
    path: PathBuf,
}

impl CcsSource {
    /// 只读打开（不写入、不复制整库，也不读取账户/密钥表）。
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("只读打开 CCS 库失败: {}", path.display()))?;
        // 与正在写入的 CCS 共存：WAL 下读取是一致快照，等锁而不是立即报忙。
        conn.busy_timeout(Duration::from_millis(5000))?;
        // 一致读事务（计划 §5.1）：结构探测、明细与日汇总必须在**同一个**读
        // 快照里读取——否则 CCS 正在归并/清理时，列探测与行读取可能看到不同的
        // 库状态（例如读到一半明细被归并删除，日汇总却是旧的）。
        conn.execute_batch("BEGIN")
            .context("开启 CCS 只读事务失败")?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 结构探测：返回版本、表与缺失列（只读，不修改来源）。
    pub fn schema(&self) -> Result<CcsSchema> {
        let user_version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        let mut tables = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
            for r in stmt.query_map([], |r| r.get::<_, String>(0))? {
                tables.push(r?);
            }
        }
        let requests_table = tables.iter().any(|t| t == "proxy_request_logs");
        let rollups_table = tables.iter().any(|t| t == "usage_daily_rollups");
        let mut missing_columns = Vec::new();
        if requests_table {
            for col in REQUIRED_REQUEST_COLUMNS {
                if !self.has_column("proxy_request_logs", col)? {
                    missing_columns.push(format!("proxy_request_logs.{col}"));
                }
            }
        }
        if rollups_table {
            for col in REQUIRED_ROLLUP_COLUMNS {
                if !self.has_column("usage_daily_rollups", col)? {
                    missing_columns.push(format!("usage_daily_rollups.{col}"));
                }
            }
        }
        Ok(CcsSchema {
            user_version,
            requests_table,
            rollups_table,
            missing_columns,
        })
    }

    fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
        let mut found = false;
        for r in stmt.query_map([], |r| r.get::<_, String>(1))? {
            if r?.eq_ignore_ascii_case(column) {
                found = true;
            }
        }
        Ok(found)
    }

    /// 读取**用量白名单列**。列顺序与下方映射一一对应：
    /// 0 request_id, 1 app_type, 2 model, 3 session_id, 4 pricing_model,
    /// 5 input, 6 output, 7 cache_read, 8 cache_creation, 9 total_cost_usd,
    /// 10 created_at, 11 data_source, 12 status_code, 13 input_token_semantics。
    ///
    /// `request_model`（客户端别名）**不读取**：TokenScope 只保存建立去重与核对
    /// 所需的最小元数据，模型身份与计价口径都不从客户端别名猜测（计划 §5.2）；
    /// 来源库里的原值不受影响。
    fn read_requests(&self) -> Result<Vec<RawRequest>> {
        let opt = |name: &str| {
            if self.has_column("proxy_request_logs", name).unwrap_or(false) {
                name.to_string()
            } else {
                "NULL".to_string()
            }
        };
        let sql = format!(
            "SELECT request_id, app_type, model, {}, {}, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, {}, created_at,
                    COALESCE(data_source, 'proxy'), {}, input_token_semantics
             FROM proxy_request_logs",
            opt("session_id"),
            opt("pricing_model"),
            opt("total_cost_usd"),
            opt("status_code"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(RawRequest {
                request_id: r.get(0)?,
                app_type: r.get(1)?,
                model: r.get(2)?,
                session_id: r.get(3)?,
                pricing_model: r.get(4)?,
                input_tokens: r.get(5)?,
                output_tokens: r.get(6)?,
                cache_read_tokens: r.get(7)?,
                cache_creation_tokens: r.get(8)?,
                total_cost_usd: r.get(9)?,
                created_at: r.get(10)?,
                data_source: r.get(11)?,
                status_code: r.get(12)?,
                input_token_semantics: r.get(13)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 读取日汇总（白名单列），列顺序同 [`RawRollup`]。
    ///
    /// `request_model` 是 CCS 日汇总**完整主键**的组成部分（同一模型下不同
    /// 客户端别名各自成行）：漏读它会把两条合法汇总折叠成一条、丢掉用量。
    fn read_rollups(&self) -> Result<Vec<RawRollup>> {
        let opt = |name: &str| {
            if self
                .has_column("usage_daily_rollups", name)
                .unwrap_or(false)
            {
                name.to_string()
            } else {
                "NULL".to_string()
            }
        };
        let sql = format!(
            "SELECT date, app_type, provider_id, model, {}, {}, request_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_token_semantics, {}
             FROM usage_daily_rollups",
            opt("request_model"),
            opt("pricing_model"),
            opt("total_cost_usd"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(RawRollup {
                date: r.get(0)?,
                app_type: r.get(1)?,
                provider_id: r.get(2)?,
                model: r.get(3)?,
                request_model: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                pricing_model: r.get(5)?,
                request_count: r.get(6)?,
                input_tokens: r.get(7)?,
                output_tokens: r.get(8)?,
                cache_read_tokens: r.get(9)?,
                cache_creation_tokens: r.get(10)?,
                input_token_semantics: r.get(11)?,
                total_cost_usd: r.get(12)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

/// 必需列（缺列 = 不支持的来源版本，明确报错而不是猜列名）。
const REQUIRED_REQUEST_COLUMNS: &[&str] = &[
    "request_id",
    "app_type",
    "model",
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_creation_tokens",
    "created_at",
    "data_source",
    "input_token_semantics",
];
const REQUIRED_ROLLUP_COLUMNS: &[&str] = &[
    "date",
    "app_type",
    "provider_id",
    "model",
    "request_count",
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_creation_tokens",
    "input_token_semantics",
];

/// 来源库结构探测结果（诊断与预览展示，字段蛇形命名同其它 IPC 载荷）。
#[derive(Debug, Clone, Serialize)]
pub struct CcsSchema {
    /// 来源库 `PRAGMA user_version`（仅诊断，不作为支持性判据）。
    pub user_version: i64,
    pub requests_table: bool,
    pub rollups_table: bool,
    pub missing_columns: Vec<String>,
}

#[derive(Debug, Clone)]
struct RawRequest {
    request_id: Option<String>,
    app_type: String,
    model: String,
    session_id: Option<String>,
    pricing_model: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    total_cost_usd: Option<String>,
    created_at: i64,
    data_source: String,
    status_code: Option<i64>,
    input_token_semantics: i64,
}

#[derive(Debug, Clone)]
struct RawRollup {
    date: String,
    app_type: String,
    provider_id: String,
    model: String,
    /// CCS 完整主键的组成部分（同模型下不同客户端别名各自成行）。
    request_model: String,
    pricing_model: Option<String>,
    request_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    input_token_semantics: i64,
    total_cost_usd: Option<String>,
}

/// 预览（**不写入任何用量**）。
///
/// 字段一律**蛇形**命名：与项目既有 IPC 载荷（`SummaryReport`/`EventRow`）同
/// 一套约定，前端 `types.ts` 逐字段对应。早期误加的 camelCase 改名会让前端
/// 读到 `undefined`（预览计数与 plan_id 全部拿不到），这里不再使用。
#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    /// 提交时原样回传（一次性、有有效期、绑定历史 generation）。
    pub plan_id: String,
    pub logical_source: String,
    pub source_path: String,
    pub source_schema: String,
    /// 日汇总的来源统计时区假设（CCS 用本地日生成日键，库内日期不携带时区）。
    pub source_day_timezone: String,
    /// 预览基于的历史库 generation；提交时不一致即要求重新预览。
    pub history_generation: u64,
    pub generated_at: String,
    pub expires_in_seconds: u64,
    pub requests_total: u64,
    pub requests_importable: u64,
    pub requests_skipped_other_app: u64,
    pub requests_skipped_duplicate_of_proxy: u64,
    pub requests_rejected: u64,
    pub rejected_reasons: Vec<String>,
    pub would_insert: u64,
    pub would_update: u64,
    pub would_unchanged: u64,
    pub would_conflict: u64,
    pub would_stale: u64,
    /// 身份不足但库内存在"同应用/同模型/同用量/时间接近"候选的记录数：
    /// 只作核对线索（计划 §5.3），默认不合并、也不自动导入。
    pub would_overlap: u64,
    /// 是否存在未解决的重叠候选：为 true 时提交需要用户显式确认。
    pub overlap_unresolved: bool,
    /// 重叠候选样例（最多 5 条，供用户判断）。
    pub overlap_examples: Vec<String>,
    pub net_new_tokens: TokenCounts,
    pub rollups_total: u64,
    pub rollups_new: u64,
    pub rollups_unchanged: u64,
    pub rollups_conflicting: u64,
    /// 本期不导入的应用（列出数量，不导入）。
    pub unsupported_apps: Vec<(String, u64)>,
    /// 没有项目归属的记录数：CCS 明细没有 cwd 列，项目保持未知。
    pub records_without_project: u64,
}

/// 提交结果（可核查）。
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub run_id: i64,
    pub logical_source: String,
    pub requests_inserted: u64,
    pub requests_updated: u64,
    pub requests_unchanged: u64,
    pub requests_conflicted: u64,
    pub requests_stale: u64,
    /// 本批按"新增"导入、但存在同时间同用量重叠候选的记录数（审计用）。
    pub requests_overlap: u64,
    pub rollups_snapshotted: u64,
    pub rollups_conflicted: u64,
    pub net_new_tokens: TokenCounts,
    pub generation_after: u64,
}

struct StagedPlan {
    preview: ImportPreview,
    writes: Vec<EventWrite>,
    rollups: Vec<DailyRollupWrite>,
    created: Instant,
}

fn plans() -> &'static Mutex<HashMap<String, StagedPlan>> {
    static PLANS: OnceLock<Mutex<HashMap<String, StagedPlan>>> = OnceLock::new();
    PLANS.get_or_init(|| Mutex::new(HashMap::new()))
}

static PLAN_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 计划身份：随机命名空间 + 单调计数（与查询会话同一取舍：跨启动不得重号）。
fn new_plan_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| anyhow::anyhow!("预览身份随机源不可用: {e}"))?;
    let mut hex = String::with_capacity(32);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    let seq = PLAN_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(format!("ccs-{hex}-{seq}"))
}

/// 生成本次导入预览（不写入任何用量）。
///
/// `source_day_timezone` 是日汇总的来源统计时区假设（默认本机时区）：CCS 用
/// `date(created_at, 'unixepoch', 'localtime')` 生成日键，库内日期不携带时区，
/// 必须显式记录该假设，不能自动宣称它是 UTC。
pub fn preview(
    source: &CcsSource,
    history: &HistoryDb,
    source_day_timezone: &str,
) -> Result<ImportPreview> {
    let schema = source.schema()?;
    if !schema.requests_table {
        bail!("来源库缺少 proxy_request_logs 表：不支持该 CCS 数据版本");
    }
    if !schema.rollups_table {
        bail!("来源库缺少 usage_daily_rollups 表：不支持该 CCS 数据版本");
    }
    if !schema.missing_columns.is_empty() {
        bail!(
            "来源库缺少必需列：{}（不支持该 CCS 数据版本）",
            schema.missing_columns.join(", ")
        );
    }

    let requests = source.read_requests()?;
    let rollups = source.read_rollups()?;

    let mut preview = ImportPreview {
        plan_id: String::new(),
        logical_source: CCS_LOGICAL_SOURCE.to_string(),
        source_path: source.path().display().to_string(),
        source_schema: format!("ccs-user-version-{}", schema.user_version),
        source_day_timezone: source_day_timezone.to_string(),
        history_generation: history.generation()?,
        generated_at: Timestamp::now().to_string(),
        expires_in_seconds: PLAN_TTL.as_secs(),
        requests_total: requests.len() as u64,
        requests_importable: 0,
        requests_skipped_other_app: 0,
        requests_skipped_duplicate_of_proxy: 0,
        requests_rejected: 0,
        rejected_reasons: Vec::new(),
        would_insert: 0,
        would_update: 0,
        would_unchanged: 0,
        would_conflict: 0,
        would_stale: 0,
        would_overlap: 0,
        overlap_unresolved: false,
        overlap_examples: Vec::new(),
        net_new_tokens: TokenCounts::default(),
        rollups_total: rollups.len() as u64,
        rollups_new: 0,
        rollups_unchanged: 0,
        rollups_conflicting: 0,
        unsupported_apps: Vec::new(),
        records_without_project: 0,
    };

    let proxy_index = ProxyIndex::build(&requests);
    let mut writes: Vec<EventWrite> = Vec::new();
    let mut unsupported: HashMap<String, u64> = HashMap::new();
    for (idx, raw) in requests.iter().enumerate() {
        let Some(app) = map_app(&raw.app_type) else {
            *unsupported.entry(raw.app_type.clone()).or_default() += 1;
            preview.requests_skipped_other_app += 1;
            continue;
        };
        if proxy_index.is_duplicate_session_row(raw) {
            // 会话行与 proxy 行是同一次请求：CCS 的"有效用量"口径只保留一条。
            // 被剔除会话行的**原生别名**由 proxy 行继承（见下），否则原生采集
            // 之后会为同一请求再建一条事件。
            preview.requests_skipped_duplicate_of_proxy += 1;
            continue;
        }
        match request_write(app, raw, CCS_LOGICAL_SOURCE) {
            Ok(mut w) => {
                // 继承被剔除会话行的原生身份（跨来源去重证据不能因为去重而丢）。
                for (scheme, value) in proxy_index.aliases_for(idx) {
                    if !w.aliases.iter().any(|a| a.scheme == *scheme) {
                        w.aliases.push(EventAlias {
                            app,
                            scheme: scheme.clone(),
                            value: value.clone(),
                        });
                    }
                }
                writes.push(w);
            }
            Err(e) => {
                preview.requests_rejected += 1;
                if preview.rejected_reasons.len() < 20 {
                    preview.rejected_reasons.push(format!("{e}"));
                }
            }
        }
    }
    if writes.len() > MAX_STAGED_REQUESTS {
        bail!(
            "本次导入的请求明细过多（{} 条，上限 {MAX_STAGED_REQUESTS}）：请分批导入",
            writes.len()
        );
    }
    preview.requests_importable = writes.len() as u64;
    preview.records_without_project = writes.len() as u64;
    let mut unsupported: Vec<(String, u64)> = unsupported.into_iter().collect();
    unsupported.sort();
    preview.unsupported_apps = unsupported;

    // 试算与提交共用同一条判定（`classify_write`），预览数字不会与提交结果分叉。
    for w in &writes {
        match history.classify_write(w)? {
            WriteClass::Insert => {
                preview.would_insert += 1;
                preview.net_new_tokens = preview
                    .net_new_tokens
                    .checked_add(&w.tokens)
                    .ok_or_else(|| anyhow::anyhow!("净新增用量溢出可表示范围"))?;
            }
            WriteClass::Update => preview.would_update += 1,
            WriteClass::Unchanged => preview.would_unchanged += 1,
            WriteClass::Conflict => preview.would_conflict += 1,
            WriteClass::Stale => preview.would_stale += 1,
            WriteClass::OverlapCandidate => {
                // 身份不足但库内有"同时间同用量"的候选：只作线索，不合并。
                preview.would_overlap += 1;
                if preview.overlap_examples.len() < 5 {
                    preview.overlap_examples.push(format!(
                        "{} / {} / {} / {}",
                        raw_app_label(w.app),
                        w.model_raw,
                        w.ts,
                        w.tokens.total()
                    ));
                }
            }
        }
    }
    preview.overlap_unresolved = preview.would_overlap > 0;

    // 日汇总：与库内同键快照对比（内容不同且来源无可验证修订 → 冲突）。
    let mut rollup_writes = Vec::new();
    for raw in &rollups {
        let Some(app) = map_app(&raw.app_type) else {
            continue;
        };
        if let Ok(w) = rollup_write(app, raw, source_day_timezone) {
            rollup_writes.push(w);
        }
    }
    for r in &rollup_writes {
        match history.classify_rollup(r)? {
            RollupClass::New => preview.rollups_new += 1,
            RollupClass::Unchanged => preview.rollups_unchanged += 1,
            RollupClass::Conflicting => preview.rollups_conflicting += 1,
        }
    }

    let plan_id = new_plan_id()?;
    preview.plan_id = plan_id.clone();
    plans().lock().unwrap_or_else(|e| e.into_inner()).insert(
        plan_id,
        StagedPlan {
            preview: preview.clone(),
            writes,
            rollups: rollup_writes,
            created: Instant::now(),
        },
    );
    log::info!(
        "CCS 导入预览：明细 {} 条（可导入 {}、其他应用 {}、会话重复 {}、拒绝 {}），日汇总 {} 条",
        preview.requests_total,
        preview.requests_importable,
        preview.requests_skipped_other_app,
        preview.requests_skipped_duplicate_of_proxy,
        preview.requests_rejected,
        preview.rollups_total
    );
    Ok(preview)
}

/// 提交一次导入：**只执行本次计划的一个批次**。计划一次性消费（双击或并发
/// 调用第二次会明确失败）；过期、历史库已变化（事务内校验 generation）或
/// 存在**未解决的重叠候选**而未获用户确认时拒绝提交并要求重新预览。
pub fn commit(
    plan_id: &str,
    history: &HistoryDb,
    policy: RollupConflictPolicy,
    allow_overlap: bool,
) -> Result<ImportReport> {
    let plan = {
        let mut guard = plans().lock().unwrap_or_else(|e| e.into_inner());
        guard
            .remove(plan_id)
            .ok_or_else(|| anyhow::anyhow!("导入计划不存在或已被提交（请重新预览）"))?
    };
    if plan.created.elapsed() > PLAN_TTL {
        bail!(
            "导入预览已过期（超过 {} 分钟），请重新预览",
            PLAN_TTL.as_secs() / 60
        );
    }
    if plan.preview.overlap_unresolved && !allow_overlap {
        // 计划 §5.3：缺少跨来源身份且存在重叠候选时不猜身份、不自动合并，
        // 也不默认把候选取值当成新请求导入。
        bail!(
            "有 {} 条来源记录与本地已有请求时间/用量相同但缺少可证实身份（重叠候选）：\
             默认不导入以免重复计费。确认这些是不同请求时可选择「按新增导入」后重新提交。",
            plan.preview.would_overlap
        );
    }
    let run = ImportRunRecord {
        logical_source: CCS_LOGICAL_SOURCE.to_string(),
        source_path: plan.preview.source_path.clone(),
        source_schema: plan.preview.source_schema.clone(),
        started_utc: plan.preview.generated_at.clone(),
        status: "committed".to_string(),
        detail: None,
        import_revision: 1,
    };
    // generation 在**事务内**再校验一次：预览之后、提交之前发生的任何采集或
    // 导入都会让整批拒绝（不拿过期预览插入重复请求）。
    let outcome = history.apply_import_batch(
        &plan.writes,
        &plan.rollups,
        policy,
        &run,
        Some(plan.preview.history_generation),
    )?;
    let generation_after = history.generation()?;
    log::info!(
        "CCS 导入完成：批次 {}，新增 {}，更新 {}，已存在 {}，冲突 {}，重叠候选 {}，日汇总落盘 {} / 冲突 {}",
        outcome.run_id,
        outcome.events.inserted,
        outcome.events.updated,
        outcome.events.unchanged,
        outcome.events.conflicts,
        outcome.events.overlap_candidates,
        outcome.rollups.inserted + outcome.rollups.replaced,
        outcome.rollups.conflicted
    );
    Ok(ImportReport {
        run_id: outcome.run_id,
        logical_source: CCS_LOGICAL_SOURCE.to_string(),
        requests_inserted: outcome.events.inserted,
        requests_updated: outcome.events.updated,
        requests_unchanged: outcome.events.unchanged,
        requests_conflicted: outcome.events.conflicts,
        requests_stale: outcome.events.stale,
        requests_overlap: outcome.events.overlap_candidates,
        rollups_snapshotted: outcome.rollups.inserted + outcome.rollups.replaced,
        rollups_conflicted: outcome.rollups.conflicted,
        net_new_tokens: history.import_run_net_tokens(outcome.run_id)?,
        generation_after,
    })
}

/// 丢弃一个预览计划（用户取消导入）：不写任何用量，也不留残留。
pub fn discard(plan_id: &str) -> bool {
    plans()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(plan_id)
        .is_some()
}

/// 当前暂存的预览计划数（诊断与测试用）。
#[doc(hidden)]
pub fn staged_plan_count() -> usize {
    plans().lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// `app_type` 映射：本期只支持 Claude 与 Codex（其他应用在预览列出但不导入）。
fn map_app(app_type: &str) -> Option<AgentKind> {
    match app_type {
        "claude" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        _ => None,
    }
}

fn raw_app_label(app: AgentKind) -> &'static str {
    match app {
        AgentKind::ClaudeCode => "claude",
        AgentKind::Codex => "codex",
    }
}

/// 非缓存输入（等价于 CCS 的 `fresh_input_sql`）。
fn fresh_input(
    app_type: &str,
    semantics: i64,
    input: u64,
    cache_read: u64,
    cache_write: u64,
) -> u64 {
    if semantics == INPUT_TOKEN_SEMANTICS_FRESH {
        return input;
    }
    if !CACHE_INCLUSIVE_APP_TYPES.contains(&app_type) {
        // Claude 的 input 本身就是非缓存输入，任何语义都不再扣减。
        return input;
    }
    if semantics == INPUT_TOKEN_SEMANTICS_TOTAL {
        if let Some(both) = cache_read.checked_add(cache_write)
            && input >= both
        {
            return input - both;
        }
    } else if input >= cache_read {
        // legacy：input 含缓存读但不含缓存写。
        return input - cache_read;
    }
    // 不满足扣减前提（如 cache 大于 input 的畸形行）时保持原值——与 CCS 的
    // ELSE 分支一致，不产出负数。
    input
}

/// 请求明细 → 历史库写入形态。
fn request_write(app: AgentKind, raw: &RawRequest, logical_source: &str) -> Result<EventWrite> {
    let request_id = raw
        .request_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("request_id 为空，无法作为来源记录"))?;
    if raw.created_at <= 0 {
        bail!("created_at 非法（{request_id}）");
    }
    if !(INPUT_TOKEN_SEMANTICS_LEGACY..=INPUT_TOKEN_SEMANTICS_FRESH)
        .contains(&raw.input_token_semantics)
    {
        bail!(
            "未知的 input_token_semantics={}（{request_id}）",
            raw.input_token_semantics
        );
    }
    if raw.model.trim().is_empty() {
        bail!("模型名为空（{request_id}）");
    }
    let input = non_negative(raw.input_tokens, "input_tokens", request_id)?;
    let output = non_negative(raw.output_tokens, "output_tokens", request_id)?;
    let cache_read = non_negative(raw.cache_read_tokens, "cache_read_tokens", request_id)?;
    let cache_write = non_negative(
        raw.cache_creation_tokens,
        "cache_creation_tokens",
        request_id,
    )?;
    let tokens = TokenCounts {
        input: fresh_input(
            &raw.app_type,
            raw.input_token_semantics,
            input,
            cache_read,
            cache_write,
        ),
        output,
        cache_write,
        cache_read,
    };
    let ts = Timestamp::new(raw.created_at, 0)
        .map_err(|e| anyhow::anyhow!("created_at 无法解释为时间戳（{request_id}）: {e}"))?;
    let source_cost = raw
        .total_cost_usd
        .as_deref()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite());
    // 来源记录键：逻辑来源 + 应用 + data_source + CCS 原始 request_id。
    let source_key = format!(
        "{logical_source}|{}|{}|{request_id}",
        raw.app_type, raw.data_source
    );
    // Claude 会话导入的 request_id 形如 `session:<message.id>`，与原生采集的
    // `(session_id, message.id)` 合起来正是同一身份 → 跨来源绑定到同一事件。
    // Codex 的 `token_count` 不带请求标识，但 `(session_id, 原始模型, 四桶)`
    // 的保守重播身份两侧同构：把它登记为别名，先导入后采集与先采集后导入
    // 都命中同一事件（并集与顺序无关）；缺 session_id 时身份不足，不猜测，
    // 只按来源键去重、库内有同时间同用量候选时列为重叠候选。
    let mut aliases = Vec::new();
    if let Some(message_id) = request_id.strip_prefix(SESSION_REQUEST_ID_PREFIX)
        && let Some(session_id) = raw.session_id.as_deref().filter(|s| !s.is_empty())
        && !message_id.is_empty()
    {
        aliases.push(EventAlias {
            app,
            scheme: "claude-message".to_string(),
            value: format!("{session_id}|{message_id}"),
        });
    } else if app == AgentKind::Codex
        && let Some(session_id) = raw.session_id.as_deref().filter(|s| !s.is_empty())
    {
        aliases.push(EventAlias {
            app,
            scheme: "codex-usage".to_string(),
            value: codex_usage_alias_value(session_id, &raw.model, &tokens),
        });
    }
    Ok(EventWrite {
        event_key: source_key.clone(),
        app,
        ts,
        model_raw: raw.model.clone(),
        model_identity: crate::model_identity::ModelIdentity::parse(&raw.model).identity_key(),
        session_id: raw.session_id.clone().filter(|s| !s.is_empty()),
        record_id: request_id
            .strip_prefix(SESSION_REQUEST_ID_PREFIX)
            .map(str::to_string),
        // CCS 明细没有 cwd/project 列：项目保持未知，不按模型或日期猜分配。
        project_key: None,
        session_initial_cwd: None,
        event_cwd: None,
        tokens,
        precedence: WritePrecedence::CcsImport,
        // 来源没有观察时间，用事件时间：同一条记录重复导入内容相同即跳过，
        // 不会被"后导入的时间更晚"误判为更新版本。
        observed_at: ts,
        aliases,
        origins: vec![EventOrigin {
            origin_kind: ORIGIN_KIND_CCS_REQUEST.to_string(),
            origin_key: source_key,
            app,
            import_run_id: None,
            parser_revision: crate::history::PARSER_REVISION,
            source_model_raw: Some(raw.model.clone()),
            pricing_model: raw.pricing_model.clone(),
            source_cost_usd: source_cost,
            // CCS 的 created_at 是 Unix 秒：如实标记秒级精度，不伪造纳秒。
            ts_precision_seconds: true,
        }],
    })
}

/// 日汇总 → 快照写入形态（归一化到 fresh 输入口径）。
fn rollup_write(
    app: AgentKind,
    raw: &RawRollup,
    source_day_timezone: &str,
) -> Result<DailyRollupWrite> {
    if raw.date.trim().is_empty() {
        bail!("日汇总缺少日期");
    }
    if raw.request_count < 0 {
        bail!("日汇总请求数为负（{}）", raw.date);
    }
    let app_type = match app {
        AgentKind::ClaudeCode => "claude",
        AgentKind::Codex => "codex",
    };
    let input = non_negative(raw.input_tokens, "input_tokens", &raw.date)?;
    let cache_read = non_negative(raw.cache_read_tokens, "cache_read_tokens", &raw.date)?;
    let cache_write = non_negative(
        raw.cache_creation_tokens,
        "cache_creation_tokens",
        &raw.date,
    )?;
    Ok(DailyRollupWrite {
        logical_source: CCS_LOGICAL_SOURCE.to_string(),
        day: raw.date.clone(),
        source_tz: source_day_timezone.to_string(),
        app,
        provider_id: raw.provider_id.clone(),
        model: raw.model.clone(),
        request_model: raw.request_model.clone(),
        pricing_model: raw.pricing_model.clone().unwrap_or_default(),
        request_count: raw.request_count as u64,
        tokens: TokenCounts {
            input: fresh_input(
                app_type,
                raw.input_token_semantics,
                input,
                cache_read,
                cache_write,
            ),
            output: non_negative(raw.output_tokens, "output_tokens", &raw.date)?,
            cache_write,
            cache_read,
        },
        // 归一化后再落库：口径与 CCS rollup 的 `INPUT_TOKEN_SEMANTICS_FRESH` 一致。
        input_semantics: INPUT_TOKEN_SEMANTICS_FRESH,
        source_cost_usd: raw
            .total_cost_usd
            .as_deref()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite()),
        // CCS 不提供可验证修订：内容不同的同键快照一律计冲突，不自动覆盖。
        revision: 0,
    })
}

/// Codex 保守重播身份的统一构造：`request_write`（按来源行导入）与
/// `session_row_aliases`（被剔除会话行向 proxy 行转移）共用，保证同一请求
/// 两侧产出的别名值逐字节一致——否则原生采集命中不了已导入事件。
fn codex_usage_alias_value(session_id: &str, model: &str, tokens: &TokenCounts) -> String {
    format!(
        "{session_id}|{model}|{}|{}|{}|{}",
        tokens.input, tokens.output, tokens.cache_write, tokens.cache_read
    )
}

fn non_negative(v: i64, what: &str, id: &str) -> Result<u64> {
    if v < 0 {
        bail!("{what} 为负（{id}）");
    }
    Ok(v as u64)
}

/// 会话行携带的原生别名（Claude：`session:<message.id>` + `session_id`；
/// Codex：保守重播身份 `codex-usage`）。
///
/// CCS 的会话导入行本身就是原生身份的另一份证据：即便该行因为"与 proxy 行
/// 重复"被按有效用量口径剔除，它的别名也必须留给被保留的那条 proxy 行——
/// 否则 TokenScope 的原生采集之后会为同一请求再建一条事件（重复计费）。
/// Codex 的重播身份按**会话行**的归一化四桶构造（原生侧解析出的正是这些
/// 真实桶值；proxy 行的 cache 写可能因 legacy 口径缺报为 0）。
fn session_row_aliases(app: AgentKind, raw: &RawRequest) -> Vec<(String, String)> {
    let Some(session_id) = raw.session_id.as_deref().filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    match app {
        AgentKind::ClaudeCode => {
            let Some(request_id) = raw.request_id.as_deref() else {
                return Vec::new();
            };
            let Some(message_id) = request_id.strip_prefix(SESSION_REQUEST_ID_PREFIX) else {
                return Vec::new();
            };
            if message_id.is_empty() {
                return Vec::new();
            }
            vec![(
                "claude-message".to_string(),
                format!("{session_id}|{message_id}"),
            )]
        }
        AgentKind::Codex => {
            if raw.model.trim().is_empty()
                || raw.input_tokens < 0
                || raw.output_tokens < 0
                || raw.cache_read_tokens < 0
                || raw.cache_creation_tokens < 0
            {
                return Vec::new();
            }
            let tokens = TokenCounts {
                input: fresh_input(
                    &raw.app_type,
                    raw.input_token_semantics,
                    raw.input_tokens as u64,
                    raw.cache_read_tokens as u64,
                    raw.cache_creation_tokens as u64,
                ),
                output: raw.output_tokens as u64,
                cache_write: raw.cache_creation_tokens as u64,
                cache_read: raw.cache_read_tokens as u64,
            };
            vec![(
                "codex-usage".to_string(),
                codex_usage_alias_value(session_id, &raw.model, &tokens),
            )]
        }
    }
}

/// proxy 行索引：判定会话行是否与某条成功 proxy 行重复（CCS 的
/// `effective_usage_log_filter` 等价实现），并记录"被剔除的会话行把原生别名
/// 留给哪条 proxy 行"。
struct ProxyIndex<'a> {
    rows: &'a [RawRequest],
    by_app: HashMap<String, Vec<usize>>,
    /// proxy 行下标 → 该行继承的原生别名。
    inherited_aliases: HashMap<usize, Vec<(String, String)>>,
}

impl<'a> ProxyIndex<'a> {
    fn build(rows: &'a [RawRequest]) -> Self {
        let mut by_app: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, r) in rows.iter().enumerate() {
            if r.data_source != "proxy" {
                continue;
            }
            // 只统计成功的 proxy 行（CCS 同口径：2xx）。
            if !r.status_code.is_some_and(|c| (200..300).contains(&c)) {
                continue;
            }
            by_app.entry(r.app_type.clone()).or_default().push(i);
        }
        let mut index = Self {
            rows,
            by_app,
            inherited_aliases: HashMap::new(),
        };
        // 先算出会话行 → proxy 行的匹配，再把别名转移过去。
        for row in rows {
            if !SESSION_DATA_SOURCES.contains(&row.data_source.as_str()) {
                continue;
            }
            let Some(app) = map_app(&row.app_type) else {
                continue;
            };
            if let Some(proxy) = index.matching_proxy_row(row) {
                let aliases = session_row_aliases(app, row);
                if !aliases.is_empty() {
                    index
                        .inherited_aliases
                        .entry(proxy)
                        .or_default()
                        .extend(aliases);
                }
            }
        }
        index
    }

    /// 该会话行是否与某条 proxy 行是同一次请求。
    fn is_duplicate_session_row(&self, row: &RawRequest) -> bool {
        self.matching_proxy_row(row).is_some()
    }

    fn matching_proxy_row(&self, row: &RawRequest) -> Option<usize> {
        if !SESSION_DATA_SOURCES.contains(&row.data_source.as_str()) {
            return None;
        }
        let candidates = self.by_app.get(&row.app_type)?;
        candidates.iter().copied().find(|&i| {
            let p = &self.rows[i];
            let app_match = p.app_type == row.app_type
                || (p.app_type == "claude" && row.app_type == "claude-desktop")
                || (p.app_type == "claude-desktop" && row.app_type == "claude");
            app_match
                && p.input_tokens == row.input_tokens
                && p.output_tokens == row.output_tokens
                && p.cache_read_tokens == row.cache_read_tokens
                && cache_creation_compatible(p, row)
                && (p.created_at - row.created_at).abs() <= SESSION_PROXY_DEDUP_WINDOW_SECONDS
                && (p.model.eq_ignore_ascii_case(&row.model)
                    || p.model.eq_ignore_ascii_case("unknown")
                    || row.model.eq_ignore_ascii_case("unknown"))
        })
    }

    /// 该 proxy 行继承的原生别名（来自被剔除的会话行）。
    fn aliases_for(&self, proxy_index: usize) -> &[(String, String)] {
        self.inherited_aliases
            .get(&proxy_index)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// cache_creation 的三个兼容分支（与 CCS 同口径）：
/// 1. 两侧相等；
/// 2. 会话侧为 0（上游日志不暴露该字段）；
/// 3. proxy 侧为 0 且是 legacy 语义（v3.17.0 前不记录 Codex 缓存写）。
fn cache_creation_compatible(p: &RawRequest, row: &RawRequest) -> bool {
    if p.cache_creation_tokens == row.cache_creation_tokens {
        return true;
    }
    if row.cache_creation_tokens == 0 && row.data_source != "proxy" {
        return true;
    }
    p.cache_creation_tokens == 0
        && p.input_token_semantics == INPUT_TOKEN_SEMANTICS_LEGACY
        && row.data_source == "codex_session"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_request() -> RawRequest {
        RawRequest {
            request_id: Some("r1".into()),
            app_type: "claude".into(),
            model: "m".into(),
            session_id: None,
            pricing_model: None,
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_cost_usd: None,
            created_at: 1_790_000_000,
            data_source: "proxy".into(),
            status_code: Some(200),
            input_token_semantics: INPUT_TOKEN_SEMANTICS_LEGACY,
        }
    }

    #[test]
    fn fresh_input_matches_ccs_rules() {
        // fresh 语义：原样。
        assert_eq!(
            fresh_input("codex", INPUT_TOKEN_SEMANTICS_FRESH, 500, 300, 200),
            500
        );
        // legacy + cache-inclusive：只扣缓存读。
        assert_eq!(
            fresh_input("codex", INPUT_TOKEN_SEMANTICS_LEGACY, 1000, 600, 0),
            400
        );
        // total + cache-inclusive：扣缓存读与写。
        assert_eq!(
            fresh_input("codex", INPUT_TOKEN_SEMANTICS_TOTAL, 1000, 300, 200),
            500
        );
        // Claude 的 input 已是非缓存输入：任何语义都不扣。
        assert_eq!(
            fresh_input("claude", INPUT_TOKEN_SEMANTICS_LEGACY, 200, 5000, 0),
            200
        );
        assert_eq!(
            fresh_input("claude", INPUT_TOKEN_SEMANTICS_TOTAL, 200, 5000, 100),
            200
        );
        // 畸形行（cache > input）保持原值，不产出负数。
        assert_eq!(
            fresh_input("codex", INPUT_TOKEN_SEMANTICS_LEGACY, 100, 999, 0),
            100
        );
        assert_eq!(
            fresh_input("codex", INPUT_TOKEN_SEMANTICS_TOTAL, 100, 300, 200),
            100
        );
    }

    #[test]
    fn app_mapping_is_explicit() {
        assert_eq!(map_app("claude"), Some(AgentKind::ClaudeCode));
        assert_eq!(map_app("codex"), Some(AgentKind::Codex));
        for other in ["gemini", "opencode", "grokbuild", "mcode", "pi", "unknown"] {
            assert_eq!(map_app(other), None, "{other} 本期不导入");
        }
    }

    #[test]
    fn session_duplicate_detection_matches_ccs() {
        let proxy = RawRequest {
            request_id: Some("proxy-1".into()),
            app_type: "codex".into(),
            model: "gpt-5.6-sol".into(),
            session_id: Some("s".into()),
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 10,
            total_cost_usd: Some("0.1".into()),
            ..base_request()
        };
        let mut session = RawRequest {
            request_id: Some("session:abc".into()),
            data_source: "codex_session".into(),
            status_code: None,
            ..proxy.clone()
        };
        let rows = vec![proxy.clone(), session.clone()];
        assert!(
            ProxyIndex::build(&rows).is_duplicate_session_row(&session),
            "指纹相同的会话行视为同一次请求"
        );

        // 时间超出窗口 → 不是同一次请求。
        session.created_at = proxy.created_at + SESSION_PROXY_DEDUP_WINDOW_SECONDS + 1;
        let rows2 = vec![proxy.clone(), session.clone()];
        assert!(!ProxyIndex::build(&rows2).is_duplicate_session_row(&session));

        // 模型不同 → 不是同一次请求。
        session.created_at = proxy.created_at;
        session.model = "gpt-5.5".into();
        let rows3 = vec![proxy.clone(), session.clone()];
        assert!(!ProxyIndex::build(&rows3).is_duplicate_session_row(&session));

        // proxy 行本身不参与"被去重"。
        let rows4 = vec![proxy.clone()];
        assert!(!ProxyIndex::build(&rows4).is_duplicate_session_row(&proxy));
    }

    #[test]
    fn session_request_id_prefix_is_validated() {
        let raw = RawRequest {
            request_id: Some("session:msg-1".into()),
            app_type: "claude".into(),
            model: "claude-opus-5-5".into(),
            session_id: Some("sess-1".into()),
            data_source: "session_log".into(),
            status_code: None,
            ..base_request()
        };
        let w = request_write(AgentKind::ClaudeCode, &raw, CCS_LOGICAL_SOURCE).unwrap();
        assert_eq!(w.aliases.len(), 1);
        assert_eq!(w.aliases[0].scheme, "claude-message");
        assert_eq!(w.aliases[0].value, "sess-1|msg-1");
        assert_eq!(w.record_id.as_deref(), Some("msg-1"));
        assert!(w.project_key.is_none(), "CCS 明细没有项目归属");
        assert!(w.origins[0].ts_precision_seconds, "秒级精度如实标记");

        // 非会话前缀（proxy 行）不猜测身份。
        let mut proxy = raw.clone();
        proxy.request_id = Some("proxy-xyz".into());
        proxy.data_source = "proxy".into();
        let w = request_write(AgentKind::ClaudeCode, &proxy, CCS_LOGICAL_SOURCE).unwrap();
        assert!(w.aliases.is_empty());
        assert!(w.record_id.is_none());

        // 缺 session_id 时同样不猜测。
        let mut no_session = raw.clone();
        no_session.session_id = None;
        let w = request_write(AgentKind::ClaudeCode, &no_session, CCS_LOGICAL_SOURCE).unwrap();
        assert!(w.aliases.is_empty());
    }

    #[test]
    fn invalid_rows_are_rejected() {
        for mutate in [
            |r: &mut RawRequest| r.input_tokens = -1,
            |r: &mut RawRequest| r.created_at = 0,
            |r: &mut RawRequest| r.model = "  ".into(),
            |r: &mut RawRequest| r.input_token_semantics = 7,
            |r: &mut RawRequest| r.request_id = None,
        ] {
            let mut raw = base_request();
            mutate(&mut raw);
            assert!(
                request_write(AgentKind::ClaudeCode, &raw, CCS_LOGICAL_SOURCE).is_err(),
                "畸形行必须被拒绝而不是猜测归一"
            );
        }
    }
}
