//! CCS 历史补充与来源选择：真实库一致副本对账（本机本地，#[ignore] + 显式环境变量门控）。
//!
//! 计划：`docs/plans/active/2026-11-09-ccs-history-delivery-repair.md` §3 T03。
//!
//! 边界与约定：
//! - 真实 TS `~/.tokenscope/history.db` 与真实 CCS `~/.cc-switch/cc-switch.db`
//!   只以 `SQLITE_OPEN_READ_ONLY` 打开，再经 **SQLite backup API** 取得一致副本
//!   （主文件拷贝会丢 WAL；本机真实库 WAL 曾达 200MB）。原始库不写入、不迁移、
//!   不 checkpoint；`CcsSource`、历史库打开（迁移/WAL）等一切写路径只发生在
//!   副本上。
//! - 产品查询走真实入口 `begin_query → query_summary`（`history_path` 指向副本，
//!   来源目录指向空隔离目录——不扫描任何真实会话；缓存、价格索引、价格文件
//!   副本全部落在输出目录）。
//! - 独立核算不经产品选择代码：直接读副本 SQL + `ModelIdentity` 重算桶键，按
//!   `docs/stats-semantics.md` §5.1 同一规则分类 A/B/C（明细桶按每条汇总行的
//!   `source_tz` 分桶，与 `collect_detail_buckets` 一致），与产品
//!   `RollupCoverage`/总量互证。
//! - 冻结 CCS 副本连续 preview→commit 两次：第二次必须零增量（不新增请求、
//!   不动汇总、净 token 为 0、generation 与事件数不变）。
//! - 证据（脱敏 JSON：日期/模型身份/计数，不含路径、密钥、对话内容）写入输出
//!   目录；数据库副本在断言通过后由本探针删除，脱敏证据保留。
//!
//! 运行（本机本地，不进 CI）：
//! ```text
//! $env:TOKENSCOPE_CCS_RECONCILE = "1"
//! $env:TOKENSCOPE_CCS_RECONCILE_OUT = "<qa-artifacts/ccs-history-delivery/<时间戳>>"
//! python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope `
//!   --test ccs_history_delivery_reconciliation -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use tokenscope::aggregate::GroupBy;
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::model_identity::ModelIdentity;
use tokenscope::query;
use tokenscope::report::SummaryOptions;

const TZ: &str = "Asia/Shanghai";
/// 旧审计时点（2026-11-09）的参考数字：只记录差值，不作为断言阈值。
const AUDIT_NO_DETAIL_BUCKETS: u64 = 61;
const AUDIT_NO_DETAIL_TOKENS: u64 = 4_337_748_027;
const AUDIT_NO_DETAIL_REQUESTS: u64 = 24_100;

#[test]
#[ignore = "需要 TOKENSCOPE_CCS_RECONCILE=1 与输出目录；只读真实库 + backup 副本，仅本机对账使用"]
fn ccs_history_delivery_real_copy_reconciliation() {
    if std::env::var("TOKENSCOPE_CCS_RECONCILE").ok().as_deref() != Some("1") {
        panic!("必须显式设置 TOKENSCOPE_CCS_RECONCILE=1 才允许读取真实库");
    }
    let out_dir = PathBuf::from(
        std::env::var("TOKENSCOPE_CCS_RECONCILE_OUT")
            .expect("必须设置 TOKENSCOPE_CCS_RECONCILE_OUT（隔离证据目录）"),
    );
    std::fs::create_dir_all(&out_dir).unwrap();

    let ts_src = tokenscope::history::history_file_path().unwrap();
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap();
    let ccs_src = PathBuf::from(&home).join(".cc-switch").join("cc-switch.db");
    assert!(ts_src.is_file(), "本机没有 {}", ts_src.display());
    assert!(ccs_src.is_file(), "本机没有 {}", ccs_src.display());

    // ---- 一致副本：只读源 + SQLite backup API（含 WAL 已提交内容） ----
    let ts_copy = out_dir.join("ts-history.db");
    let ts_import = out_dir.join("ts-import.db");
    let ccs_copy = out_dir.join("ccs-source.db");
    backup_into(&ts_src, &ts_copy);
    backup_into(&ts_src, &ts_import);
    backup_into(&ccs_src, &ccs_copy);

    // 价格文件复制到隔离目录（只读来源；探针的任何写路径都落不回真实数据目录）。
    let data_dir = tokenscope::report::data_dir().unwrap();
    let pricing_dir = out_dir.join("pricing");
    std::fs::create_dir_all(&pricing_dir).unwrap();
    let mut pricing_files = Vec::new();
    for name in [
        "pricing.toml",
        "pricing-openrouter.json",
        "pricing-modelsdev.json",
    ] {
        let src = data_dir.join(name);
        let dst = pricing_dir.join(name);
        if src.is_file() {
            std::fs::copy(&src, &dst).unwrap();
        }
        pricing_files.push(dst);
    }
    // 空来源目录：产品查询的采集管线在这里发现零文件，不触碰任何真实会话日志。
    for name in ["no-claude-logs", "no-codex-logs"] {
        std::fs::create_dir_all(out_dir.join(name)).unwrap();
    }
    let empty_logs = out_dir.join("no-claude-logs");

    // ---- 阶段一：pristine 副本的产品查询与独立核算互证 ----
    let pristine_user_version = copy_user_version(&ts_copy);
    let pristine = run_product_query(&out_dir, &ts_copy, &pricing_files, &empty_logs);
    let pristine_indep = independent_accounting(&ts_copy);
    let pristine_rollups = read_ts_rollups(&ts_copy);

    assert_eq!(
        pristine.coverage.rollup_buckets, pristine_indep.rollup_only_count,
        "无明细桶数：产品 vs 独立核算"
    );
    assert_eq!(
        pristine.coverage.detail_selected_buckets, pristine_indep.a_count,
        "A 类桶数：产品 vs 独立核算"
    );
    assert_eq!(
        pristine.coverage.covered_buckets, pristine_indep.b_count,
        "B 类桶数：产品 vs 独立核算"
    );
    assert_eq!(
        pristine.coverage.unresolved_buckets, pristine_indep.c_count,
        "C 类桶数：产品 vs 独立核算"
    );
    assert_eq!(
        pristine.coverage.conflict_kinds, pristine_indep.conflict_kinds,
        "冲突类型：产品 vs 独立核算"
    );
    assert_eq!(
        pristine.totals.requests, pristine_indep.expected_requests,
        "全部时间 Claude 总请求数：产品 vs 独立核算（含无明细汇总桶）"
    );
    assert_tokens_eq(
        pristine.totals.tokens,
        &pristine_indep.expected_tokens,
        "pristine 总量",
    );

    assert!(
        pristine_indep.rollup_only_count > 0,
        "真实库必须存在无明细汇总桶（会话已删除、历史仍统计的前提）"
    );

    // ---- 阶段二：冻结 CCS 副本连续两次 preview→commit ----
    let import_user_version = copy_user_version(&ts_import);
    let history_import = tokenscope::history::HistoryDb::open(&ts_import).unwrap();
    let events_before = history_import.event_count().unwrap();
    let generation_before = history_import.generation().unwrap();

    let first = import_round(&ts_import, &ccs_copy);
    let events_after_first = history_import.event_count().unwrap();
    let generation_after_first = history_import.generation().unwrap();

    let second = import_round(&ts_import, &ccs_copy);
    let events_after_second = history_import.event_count().unwrap();
    let generation_after_second = history_import.generation().unwrap();

    assert_eq!(second.preview.would_insert, 0, "第二次预览不得有待插入请求");
    assert_eq!(second.preview.rollups_new, 0, "第二次预览不得有新日汇总");
    assert_eq!(second.commit.requests_inserted, 0, "第二次导入不得新增请求");
    assert_eq!(
        second.commit.rollups_snapshotted, 0,
        "第二次导入不得落盘新日汇总"
    );
    assert_eq!(
        second.commit.net_new_tokens.total(),
        0,
        "第二次导入净 token 必须为 0"
    );
    assert_eq!(
        generation_after_second, generation_after_first,
        "第二次导入不得递增 generation"
    );
    assert_eq!(
        events_after_second, events_after_first,
        "第二次导入不得改变事件数"
    );

    // ---- 阶段三：导入后副本再走产品查询，独立核算互证 + 变化桶净变化 ----
    let post = run_product_query(&out_dir, &ts_import, &pricing_files, &empty_logs);
    let post_indep = independent_accounting(&ts_import);
    assert_eq!(
        post.coverage.rollup_buckets, post_indep.rollup_only_count,
        "导入后无明细桶数：产品 vs 独立核算"
    );
    assert_eq!(
        post.coverage.detail_selected_buckets, post_indep.a_count,
        "导入后 A 类桶数：产品 vs 独立核算"
    );
    assert_eq!(
        post.coverage.covered_buckets, post_indep.b_count,
        "导入后 B 类桶数：产品 vs 独立核算"
    );
    assert_eq!(
        post.coverage.unresolved_buckets, post_indep.c_count,
        "导入后 C 类桶数：产品 vs 独立核算"
    );
    assert_eq!(
        post.totals.requests, post_indep.expected_requests,
        "导入后总请求数：产品 vs 独立核算"
    );
    assert_tokens_eq(
        post.totals.tokens,
        &post_indep.expected_tokens,
        "post 导入后总量",
    );

    // 变化/新增的汇总桶：其用量之和不是增量；记录两侧选择与最终计入值。
    let post_rollups = read_ts_rollups(&ts_import);
    let mut changed_buckets = Vec::new();
    let mut keys: Vec<String> = pristine_rollups
        .keys()
        .cloned()
        .chain(post_rollups.keys().cloned())
        .collect();
    keys.sort();
    keys.dedup();
    for key in &keys {
        if pristine_rollups.get(key) == post_rollups.get(key) {
            continue;
        }
        let row = post_rollups.get(key);
        // changed 键为 `(day|identity)`；独立核算桶含 source_tz，取同 (day,
        // identity) 的全部桶计入值之和（本库单时区，通常一对一）。
        let counted_pre = counted_for_day_identity(&pristine_indep, key);
        let counted_post = counted_for_day_identity(&post_indep, key);
        changed_buckets.push(serde_json::json!({
            "bucket": key,
            "rollup_requests": row.map(|r| r.requests).unwrap_or(0),
            "rollup_tokens": row.map(|r| r.tokens.total()).unwrap_or(0),
            "counted_requests_pre": counted_pre.0,
            "counted_tokens_pre": counted_pre.1,
            "counted_requests_post": counted_post.0,
            "counted_tokens_post": counted_post.1,
        }));
    }

    // ---- 阶段四：已导入日汇总与 CCS 来源逐字段对账 ----
    let field_check = field_reconciliation(&ts_copy, &ccs_copy);

    // ---- 证据（脱敏）----
    let no_detail: Vec<serde_json::Value> = pristine_indep
        .buckets
        .iter()
        .filter(|b| b.selection == "rollup_only")
        .map(|b| {
            serde_json::json!({
                "day": b.day, "model": b.identity,
                "requests": b.rollup_requests,
                "tokens": b.rollup_tokens.total(),
            })
        })
        .collect();
    let overlap: Vec<serde_json::Value> = pristine_indep
        .buckets
        .iter()
        .filter(|b| b.day == "2026-10-05")
        .map(|b| {
            serde_json::json!({
                "day": b.day, "model": b.identity,
                "detail_requests": b.detail_requests,
                "detail_tokens": b.detail_tokens.total(),
                "snapshot_requests": b.snapshot_requests,
                "snapshot_tokens": b.snapshot_tokens.total(),
                "rollup_requests": b.rollup_requests,
                "rollup_tokens": b.rollup_tokens.total(),
                "boundary_unknown": b.boundary_unknown,
                "selection": b.selection,
            })
        })
        .collect();
    let audit_delta = |actual: u64, old: u64| -> i64 { actual as i64 - old as i64 };
    let evidence = serde_json::json!({
        "schema_version": 1,
        "generated_at": jiff::Timestamp::now().to_string(),
        "plan": "docs/plans/active/2026-11-09-ccs-history-delivery-repair.md T03",
        "method": "SQLite backup API 一致副本；原始 TS/CCS 库只读；产品真实查询 + 独立 SQL 核算互证",
        "copies": {
            "ts_history": "ts-history.db", "ts_import": "ts-import.db",
            "ccs_source": "ccs-source.db",
            "ts_user_version_pristine": pristine_user_version,
            "ts_user_version_import": import_user_version,
        },
        "ccs_source_tz_values": pristine_indep.source_tz_values,
        "pristine": {
            "product_totals": totals_json(&pristine.totals),
            "product_coverage": pristine.coverage_serde,
            "independent": pristine_indep.summary_json(),
            "no_detail_buckets": no_detail,
            "overlap_buckets_2026_10_05": overlap,
            "audit_delta_vs_2026_11_09": {
                "no_detail_buckets": audit_delta(pristine_indep.rollup_only_count, AUDIT_NO_DETAIL_BUCKETS),
                "no_detail_tokens": audit_delta(pristine_indep.rollup_only_tokens_total(), AUDIT_NO_DETAIL_TOKENS),
                "no_detail_requests": audit_delta(pristine_indep.rollup_only_requests_total(), AUDIT_NO_DETAIL_REQUESTS),
            },
        },
        "import_round": {
            "first_preview": first.preview_json,
            "first_commit": first.commit_json,
            "first_used_allow_overlap": first.used_allow_overlap,
            "second_preview": second.preview_json,
            "second_commit": second.commit_json,
            "second_zero_delta": true,
            "events_before": events_before,
            "events_after_first": events_after_first,
            "events_after_second": events_after_second,
            "generation_before": generation_before,
            "generation_after_first": generation_after_first,
            "generation_after_second": generation_after_second,
            "changed_rollup_buckets": changed_buckets,
            "post_product_coverage": post.coverage_serde,
            "post_independent": post_indep.summary_json(),
            "post_product_totals": totals_json(&post.totals),
        },
        "imported_rollups_field_check": field_check,
    });
    let evidence_path = out_dir.join("reconciliation.json");
    std::fs::write(
        &evidence_path,
        serde_json::to_string_pretty(&evidence).unwrap() + "\n",
    )
    .unwrap();
    println!("证据已写入 {}", evidence_path.display());
    println!(
        "pristine：无明细桶 {}（{} 请求 / {} token），A {}，B {}，C {}；产品总量 {} 请求 / {} token",
        pristine_indep.rollup_only_count,
        pristine_indep.rollup_only_requests_total(),
        pristine_indep.rollup_only_tokens_total(),
        pristine_indep.a_count,
        pristine_indep.b_count,
        pristine_indep.c_count,
        pristine.totals.requests,
        pristine.totals.tokens.total()
    );
    println!(
        "导入：首次 rollups_new={} / 插入 {} / 净 token {}；第二次零增量（净 0，generation 不变）",
        first.preview.rollups_new,
        first.commit.requests_inserted,
        first.commit.net_new_tokens.total()
    );

    // ---- 收尾：删除本轮创建的数据库副本，保留脱敏证据 ----
    drop(history_import);
    // 查询注册表仍持有 ts 副本的只读快照（TTL 10 分钟）：回拨后下一次 gc 回收，
    // 使 Windows 不再锁定副本文件，然后删库。
    for snap in [pristine.snapshot_id.as_str(), post.snapshot_id.as_str()] {
        query::backdate_query_for_tests(snap, query::QUERY_IDLE_TTL);
    }
    let _ = query::begin_query(&SummaryOptions {
        by: GroupBy::Model,
        agent: Some(AgentKind::ClaudeCode),
        claude_dir: Some(empty_logs.to_path_buf()),
        codex_dir: Some(out_dir.join("no-codex-logs")),
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        cache_dir: Some(out_dir.join("cache")),
        tz: Some(TZ.to_string()),
        history_path: Some(out_dir.join("gc-probe.db")),
        ..Default::default()
    });
    remove_db_copy(&ts_copy);
    remove_db_copy(&ts_import);
    remove_db_copy(&ccs_copy);
    let _ = std::fs::remove_file(out_dir.join("gc-probe.db"));
    let _ = std::fs::remove_dir_all(&pricing_dir);
    assert!(
        !ts_copy.exists() && !ts_import.exists() && !ccs_copy.exists(),
        "本轮创建的数据库副本必须清理"
    );
}

// ---------- 副本与产品查询 ----------

/// 只读打开源库，经 SQLite backup API 生成一致副本（主文件拷贝会丢 WAL）。
fn backup_into(src: &Path, dst: &Path) {
    let source = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap_or_else(|e| panic!("只读打开 {} 失败: {e}", src.display()));
    let _ = std::fs::remove_file(dst);
    let mut target = Connection::open(dst).unwrap();
    Backup::new(&source, &mut target)
        .unwrap()
        .run_to_completion(128, Duration::from_millis(20), None)
        .unwrap();
    drop(target);
    drop(source);
}

fn copy_user_version(db: &Path) -> i64 {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn totals_json(totals: &tokenscope::aggregate::Group) -> serde_json::Value {
    serde_json::json!({
        "requests": totals.requests,
        "input": totals.tokens.input,
        "output": totals.tokens.output,
        "cache_write": totals.tokens.cache_write,
        "cache_read": totals.tokens.cache_read,
    })
}

/// 产品真实查询：全部时间 / 仅 Claude / 模型维度；来源目录为空隔离目录。
fn run_product_query(
    out_dir: &Path,
    history: &Path,
    pricing_files: &[PathBuf],
    empty_logs: &Path,
) -> ProductView {
    let opts = SummaryOptions {
        by: GroupBy::Model,
        agent: Some(AgentKind::ClaudeCode),
        claude_dir: Some(empty_logs.to_path_buf()),
        codex_dir: Some(out_dir.join("no-codex-logs")),
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        cache_dir: Some(out_dir.join("cache")),
        pricing_path: Some(pricing_files[0].clone()),
        openrouter_path: Some(pricing_files[1].clone()),
        modelsdev_path: Some(pricing_files[2].clone()),
        pricing_index: Some(out_dir.join("pricing-index.json")),
        tz: Some(TZ.to_string()),
        history_path: Some(history.to_path_buf()),
        ..Default::default()
    };
    let snap = query::begin_query(&opts).unwrap();
    let report = query::query_summary(&snap.query_id).unwrap();
    let coverage_serde = serde_json::to_value(&report.rollup_coverage).unwrap();
    ProductView {
        snapshot_id: snap.query_id.clone(),
        totals: report.totals.clone(),
        coverage: report.rollup_coverage.clone(),
        coverage_serde,
    }
}

struct ProductView {
    snapshot_id: String,
    totals: tokenscope::aggregate::Group,
    coverage: tokenscope::aggregate::RollupCoverage,
    coverage_serde: serde_json::Value,
}

fn assert_tokens_eq(actual: TokenCounts, expected: &TokenCounts, label: &str) {
    assert_eq!(actual.input, expected.input, "{label}: input");
    assert_eq!(actual.output, expected.output, "{label}: output");
    assert_eq!(
        actual.cache_write, expected.cache_write,
        "{label}: cache_write"
    );
    assert_eq!(
        actual.cache_read, expected.cache_read,
        "{label}: cache_read"
    );
}

// ---------- 独立核算（不经产品选择代码） ----------

#[derive(Clone)]
struct IndepBucket {
    /// 桶键 = `(来源时区, 来源自然日, 等价模型)`（与 `collect_detail_buckets` 一致）；
    /// 本探针证据里以 `(day|identity)` 呈现（真实库单时区）。
    #[allow(dead_code)]
    source_tz: String,
    day: String,
    identity: String,
    /// 汇总侧（整桶多行相加）。
    rollup_requests: u64,
    rollup_tokens: TokenCounts,
    /// 明细侧（该桶全部事件）。
    detail_requests: u64,
    detail_tokens: TokenCounts,
    /// 快照范围内明细（边界不可确认时 = 全部明细，与产品口径一致）。
    snapshot_requests: u64,
    snapshot_tokens: TokenCounts,
    boundary_unknown: bool,
    selection: &'static str,
}

impl IndepBucket {
    fn rollup_tokens_total(&self) -> u64 {
        self.rollup_tokens.total()
    }
    /// §5.1：该桶最终计入统计的 (请求数, token 四桶)。
    fn counted(&self) -> (u64, u64, u64, u64, u64) {
        match self.selection {
            // 汇总计入 + 快照外新增按明细另计。
            "rollup_only" | "b_rollup_supplements" => {
                let new_req = self.detail_requests - self.snapshot_requests;
                (
                    self.rollup_requests + new_req,
                    self.rollup_tokens.input
                        + (self.detail_tokens.input - self.snapshot_tokens.input),
                    self.rollup_tokens.output
                        + (self.detail_tokens.output - self.snapshot_tokens.output),
                    self.rollup_tokens.cache_write
                        + (self.detail_tokens.cache_write - self.snapshot_tokens.cache_write),
                    self.rollup_tokens.cache_read
                        + (self.detail_tokens.cache_read - self.snapshot_tokens.cache_read),
                )
            }
            // A / C：按明细；B 的快照外新增已并入上分支。
            _ => (
                self.detail_requests,
                self.detail_tokens.input,
                self.detail_tokens.output,
                self.detail_tokens.cache_write,
                self.detail_tokens.cache_read,
            ),
        }
    }
}

struct IndepResult {
    buckets: Vec<IndepBucket>,
    rollup_only_count: u64,
    a_count: u64,
    b_count: u64,
    c_count: u64,
    conflict_kinds: Vec<String>,
    expected_requests: u64,
    expected_tokens: TokenCounts,
    source_tz_values: Vec<String>,
    excluded_bad_tz_rows: u64,
}

impl IndepResult {
    fn rollup_only_tokens_total(&self) -> u64 {
        self.buckets
            .iter()
            .filter(|b| b.selection == "rollup_only")
            .map(|b| b.rollup_tokens_total())
            .sum()
    }
    fn rollup_only_requests_total(&self) -> u64 {
        self.buckets
            .iter()
            .filter(|b| b.selection == "rollup_only")
            .map(|b| b.rollup_requests)
            .sum()
    }
    fn summary_json(&self) -> serde_json::Value {
        serde_json::json!({
            "bucket_count": self.buckets.len(),
            "rollup_only": {
                "buckets": self.rollup_only_count,
                "requests": self.rollup_only_requests_total(),
                "tokens": self.rollup_only_tokens_total(),
            },
            "a_detail_selected": self.a_count,
            "b_rollup_supplements": self.b_count,
            "c_crossed_conflict": self.c_count,
            "conflict_kinds": self.conflict_kinds,
            "expected_requests": self.expected_requests,
            "expected_tokens": self.expected_tokens.total(),
            "excluded_bad_tz_rows": self.excluded_bad_tz_rows,
        })
    }
}

/// 按 `(day|identity)` 汇总该日模型桶的最终计入值（跨 source_tz 相加）。
fn counted_for_day_identity(result: &IndepResult, key: &str) -> (u64, u64) {
    let mut requests = 0u64;
    let mut tokens = 0u64;
    for b in &result.buckets {
        if format!("{}|{}", b.day, b.identity) == *key {
            let (r, i, o, cw, cr) = b.counted();
            requests += r;
            tokens += i + o + cw + cr;
        }
    }
    (requests, tokens)
}

/// 在 TS 副本上独立重算 §5.1 三向选择（SQL 读数 + ModelIdentity 桶键）。
///
/// 明细桶按每条汇总行的 `source_tz` 分桶（与 `collect_detail_buckets` 相同）：
/// 汇总桶键 `(source_tz, day, app, identity)`；不在任何汇总时区的明细事件永远
/// 按明细计入（它们不参与任何覆盖判定）。
fn independent_accounting(ts_copy: &Path) -> IndepResult {
    let conn = Connection::open_with_flags(ts_copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();

    // 汇总行 → 桶（整桶多行相加；覆盖起点取最早快照时刻，与产品一致）。
    struct RollupRow {
        source_tz: String,
        day: String,
        model: String,
        requests: u64,
        tokens: TokenCounts,
        import_run_id: Option<i64>,
    }
    let mut rollup_rows = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT source_tz, day, model, request_count, input_tokens, output_tokens,
                        cache_write_tokens, cache_read_tokens, import_run_id
                 FROM ccs_daily_usage WHERE app = 'claude-code'",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok(RollupRow {
                    source_tz: r.get(0)?,
                    day: r.get(1)?,
                    model: r.get(2)?,
                    requests: r.get::<_, i64>(3)?.max(0) as u64,
                    tokens: TokenCounts {
                        input: u64_text(r, 4),
                        output: u64_text(r, 5),
                        cache_write: u64_text(r, 6),
                        cache_read: u64_text(r, 7),
                        cache_write_5m: 0,
                        cache_write_1h: 0,
                        cache_write_split_known: false,
                    },
                    import_run_id: r.get(8)?,
                })
            })
            .unwrap();
        for row in rows {
            rollup_rows.push(row.unwrap());
        }
    }
    // 批次时刻：修订 >= 2 才代表"快照建立时刻"语义（与产品 boundary_known 一致）。
    let run_times: BTreeMap<i64, (Option<jiff::Timestamp>, i64)> = {
        let mut stmt = conn
            .prepare("SELECT id, started_utc, import_revision FROM import_runs")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                let started: Option<String> = r.get(1)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    (
                        started.and_then(|s| s.trim().parse::<jiff::Timestamp>().ok()),
                        r.get::<_, i64>(2)?,
                    ),
                ))
            })
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    };

    #[derive(Default)]
    struct RollupAgg {
        requests: u64,
        tokens: TokenCounts,
        since: Option<jiff::Timestamp>,
        boundary_unknown: bool,
    }
    let mut rollups: BTreeMap<(String, String, String), RollupAgg> = BTreeMap::new();
    let mut source_tz_values: Vec<String> = Vec::new();
    let mut excluded_bad_tz_rows = 0u64;
    for row in &rollup_rows {
        if jiff::tz::TimeZone::get(&row.source_tz).is_err() {
            // 与产品一致：认不出来的来源时区，行整体不参与（不重切日期）。
            excluded_bad_tz_rows += 1;
            continue;
        }
        if !source_tz_values.contains(&row.source_tz) {
            source_tz_values.push(row.source_tz.clone());
        }
        let identity = ModelIdentity::parse(&row.model).identity_key();
        let agg = rollups
            .entry((row.source_tz.clone(), row.day.clone(), identity))
            .or_default();
        agg.requests += row.requests;
        agg.tokens = agg.tokens.checked_add(&row.tokens).unwrap();
        let known = row
            .import_run_id
            .and_then(|id| run_times.get(&id))
            .is_some_and(|(at, rev)| rev >= &2 && at.is_some());
        match (known, row.import_run_id.and_then(|id| run_times.get(&id))) {
            (true, Some((Some(at), _))) => {
                let at = *at;
                agg.since = Some(agg.since.map_or(at, |cur: jiff::Timestamp| cur.min(at)));
            }
            _ => agg.boundary_unknown = true,
        }
    }

    // 明细事件，按每个汇总来源时区分桶；时区外的桶键留作"永远按明细计入"。
    struct EventRow {
        ts_seconds: i64,
        ts_nanos: i32,
        model_raw: String,
        tokens: TokenCounts,
    }
    let mut events = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT ts_seconds, ts_nanos, model_raw, input_tokens, output_tokens,
                        cache_write_tokens, cache_read_tokens
                 FROM usage_events WHERE app = 'claude-code'",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok(EventRow {
                    ts_seconds: r.get(0)?,
                    ts_nanos: r.get(1)?,
                    model_raw: r.get(2)?,
                    tokens: TokenCounts {
                        input: u64_text(r, 3),
                        output: u64_text(r, 4),
                        cache_write: u64_text(r, 5),
                        cache_read: u64_text(r, 6),
                        cache_write_5m: 0,
                        cache_write_1h: 0,
                        cache_write_split_known: false,
                    },
                })
            })
            .unwrap();
        for row in rows {
            events.push(row.unwrap());
        }
    }
    #[derive(Default)]
    struct DetailAgg {
        requests: u64,
        tokens: TokenCounts,
    }
    let zoned_cache: Vec<(String, jiff::tz::TimeZone)> = source_tz_values
        .iter()
        .map(|tz| (tz.clone(), jiff::tz::TimeZone::get(tz).unwrap()))
        .collect();
    // details[(tz, day, identity)]；tz_only_events = 不属于任何汇总时区的事件。
    let mut details: BTreeMap<(String, String, String), DetailAgg> = BTreeMap::new();
    let mut tz_only_requests = 0u64;
    let mut tz_only_tokens = TokenCounts::default();
    for e in &events {
        let ts = jiff::Timestamp::new(e.ts_seconds, e.ts_nanos).unwrap();
        let identity = ModelIdentity::parse(&e.model_raw).identity_key();
        let mut matched = false;
        for (tz_name, tz) in &zoned_cache {
            let day = ts.to_zoned(tz.clone()).date().to_string();
            let agg = details
                .entry((tz_name.clone(), day, identity.clone()))
                .or_default();
            agg.requests += 1;
            agg.tokens = agg.tokens.checked_add(&e.tokens).unwrap();
            matched = true;
        }
        if !matched {
            // 没有任何汇总时区（例如库中无汇总）：明细照常计入。
            tz_only_requests += 1;
            tz_only_tokens = tz_only_tokens.checked_add(&e.tokens).unwrap();
        }
    }

    // 三向选择（§5.1：只比较快照范围内明细与整桶汇总，逐项比较）。
    let mut result = IndepResult {
        buckets: Vec::new(),
        rollup_only_count: 0,
        a_count: 0,
        b_count: 0,
        c_count: 0,
        conflict_kinds: Vec::new(),
        expected_requests: 0,
        expected_tokens: TokenCounts::default(),
        source_tz_values,
        excluded_bad_tz_rows,
    };
    let mut kinds: Vec<String> = Vec::new();
    let mut keys: Vec<_> = rollups.keys().cloned().collect();
    keys.extend(details.keys().cloned());
    keys.sort();
    keys.dedup();
    for key in keys {
        let roll = rollups.get(&key);
        let detail = details.get(&key);
        let detail_requests = detail.map(|d| d.requests).unwrap_or(0);
        let detail_tokens = detail.map(|d| d.tokens).unwrap_or_default();
        let Some(roll) = roll else {
            // 只有明细、没有汇总的桶：按明细计入（与产品一致——汇总不存在就不
            // 参与选择）。
            result.expected_requests += detail_requests;
            result.expected_tokens = result.expected_tokens.checked_add(&detail_tokens).unwrap();
            continue;
        };
        let (snapshot_requests, snapshot_tokens) = if detail_requests == 0 {
            (0u64, TokenCounts::default())
        } else if roll.boundary_unknown {
            // 边界不可确认：整桶明细按快照内处理（与产品一致）。
            (detail_requests, detail_tokens)
        } else {
            let since = roll.since.unwrap();
            let mut req = 0u64;
            let mut tok = TokenCounts::default();
            for e in &events {
                let ts = jiff::Timestamp::new(e.ts_seconds, e.ts_nanos).unwrap();
                let identity = ModelIdentity::parse(&e.model_raw).identity_key();
                if identity != key.2 {
                    continue;
                }
                let day = ts
                    .to_zoned(jiff::tz::TimeZone::get(&key.0).unwrap().clone())
                    .date()
                    .to_string();
                if day == key.1 && ts <= since {
                    req += 1;
                    tok = tok.checked_add(&e.tokens).unwrap();
                }
            }
            (req, tok)
        };
        let selection: &'static str = if detail_requests == 0 {
            result.rollup_only_count += 1;
            "rollup_only"
        } else {
            let d = &snapshot_tokens;
            let r = &roll.tokens;
            let any_gt = d.input > r.input
                || d.output > r.output
                || d.cache_write > r.cache_write
                || d.cache_read > r.cache_read;
            let any_lt = d.input < r.input
                || d.output < r.output
                || d.cache_write < r.cache_write
                || d.cache_read < r.cache_read;
            let ge = snapshot_requests >= roll.requests;
            let le = snapshot_requests <= roll.requests;
            let exactly_equal = ge && le && !any_gt && !any_lt;
            if ge && !any_lt {
                result.a_count += 1;
                "a_detail_selected"
            } else if le && !any_gt && !exactly_equal {
                result.b_count += 1;
                "b_rollup_supplements"
            } else {
                result.c_count += 1;
                let kind = if any_gt && any_lt {
                    "crossed_token_buckets"
                } else {
                    "opposed_requests_tokens"
                };
                if !kinds.contains(&kind.to_string()) {
                    kinds.push(kind.to_string());
                }
                "c_crossed_conflict"
            }
        };
        result.buckets.push(IndepBucket {
            source_tz: key.0.clone(),
            day: key.1.clone(),
            identity: key.2.clone(),
            rollup_requests: roll.requests,
            rollup_tokens: roll.tokens,
            detail_requests,
            detail_tokens,
            snapshot_requests,
            snapshot_tokens,
            boundary_unknown: roll.boundary_unknown,
            selection,
        });
    }
    // 最终计入 = 各汇总桶（按选择）+ 纯明细桶 + 时区外事件。
    for b in &result.buckets {
        let (req, input, output, cw, cr) = b.counted();
        result.expected_requests += req;
        result.expected_tokens.input += input;
        result.expected_tokens.output += output;
        result.expected_tokens.cache_write += cw;
        result.expected_tokens.cache_read += cr;
    }
    result.expected_requests += tz_only_requests;
    result.expected_tokens = result.expected_tokens.checked_add(&tz_only_tokens).unwrap();
    kinds.sort();
    result.conflict_kinds = kinds;
    result
}

fn u64_text(r: &rusqlite::Row<'_>, idx: usize) -> u64 {
    let raw: String = r.get(idx).unwrap();
    raw.trim().parse::<u64>().unwrap_or(0)
}

// ---------- 导入轮次 ----------

struct ImportRound {
    preview: tokenscope::import::ccs::ImportPreview,
    commit: tokenscope::import::ccs::ImportReport,
    used_allow_overlap: bool,
    preview_json: serde_json::Value,
    commit_json: serde_json::Value,
}

/// 真实产品路径：`CcsSource::open → preview → commit`（GUI 默认 KeepExisting；
/// 遇重叠候选时先按默认拒绝，再走 GUI 的「按新增导入」显式确认路径并记录）。
fn import_round(ts_import: &Path, ccs_copy: &Path) -> ImportRound {
    let do_round = |allow: bool| -> Result<
        (
            tokenscope::import::ccs::ImportPreview,
            tokenscope::import::ccs::ImportReport,
        ),
        anyhow::Error,
    > {
        let history = tokenscope::history::HistoryDb::open(ts_import)?;
        let source = tokenscope::import::ccs::CcsSource::open(ccs_copy)?;
        let preview = tokenscope::import::ccs::preview(&source, &history, TZ)?;
        drop(source);
        let report = tokenscope::import::ccs::commit(
            &preview.plan_id,
            &history,
            tokenscope::history::RollupConflictPolicy::KeepExisting,
            allow,
        )?;
        Ok((preview, report))
    };
    let (preview, commit, used_allow_overlap) = match do_round(false) {
        Ok((p, c)) => (p, c, false),
        Err(e) if e.to_string().contains("重叠候选") => {
            println!("默认提交被重叠候选拒绝，改走 GUI「按新增导入」确认路径：{e}");
            let (p, c) = do_round(true).expect("按新增导入路径失败");
            (p, c, true)
        }
        Err(e) => panic!("导入轮次失败: {e}"),
    };
    let preview_json = serde_json::json!({
        "requests_total": preview.requests_total,
        "requests_importable": preview.requests_importable,
        "requests_skipped_other_app": preview.requests_skipped_other_app,
        "requests_skipped_duplicate_of_proxy": preview.requests_skipped_duplicate_of_proxy,
        "requests_rejected": preview.requests_rejected,
        "would_insert": preview.would_insert,
        "would_update": preview.would_update,
        "would_unchanged": preview.would_unchanged,
        "would_conflict": preview.would_conflict,
        "would_overlap": preview.would_overlap,
        "net_new_tokens": preview.net_new_tokens.total(),
        "rollups_total": preview.rollups_total,
        "rollups_new": preview.rollups_new,
        "rollups_unchanged": preview.rollups_unchanged,
        "rollups_conflicting": preview.rollups_conflicting,
    });
    let commit_json = serde_json::json!({
        "requests_inserted": commit.requests_inserted,
        "requests_updated": commit.requests_updated,
        "requests_unchanged": commit.requests_unchanged,
        "requests_conflicted": commit.requests_conflicted,
        "requests_overlap": commit.requests_overlap,
        "rollups_snapshotted": commit.rollups_snapshotted,
        "rollups_conflicted": commit.rollups_conflicted,
        "net_new_tokens": commit.net_new_tokens.total(),
        "generation_after": commit.generation_after,
    });
    ImportRound {
        used_allow_overlap,
        preview,
        commit,
        preview_json,
        commit_json,
    }
}

// ---------- 逐字段对账 ----------

/// 已导入 TS 日汇总 vs CCS 来源（app_type='claude'）逐字段一致计数。
///
/// TS 侧 token 列恒为 TEXT 十进制；CCS 侧数值列是 INTEGER——按 `Value` 读取后
/// 统一规范化为十进制文本再逐位比较。
fn field_reconciliation(ts_copy: &Path, ccs_copy: &Path) -> serde_json::Value {
    use rusqlite::types::Value as V;
    let norm = |v: V| -> String {
        match v {
            V::Null => String::new(),
            V::Integer(i) => i.to_string(),
            V::Real(f) => format!("{f}"),
            V::Text(s) => s.trim().to_string(),
            V::Blob(_) => "<blob>".to_string(),
        }
    };
    let ts = Connection::open_with_flags(ts_copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let ccs = Connection::open_with_flags(ccs_copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut ts_rows: BTreeMap<String, Vec<String>> = BTreeMap::new();
    {
        let mut stmt = ts
            .prepare(
                "SELECT day, provider_id, model, request_model, pricing_model, request_count,
                        input_tokens, output_tokens, cache_write_tokens, cache_read_tokens,
                        input_semantics
                 FROM ccs_daily_usage WHERE app = 'claude-code'",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                let key = format!(
                    "{}|{}|{}|{}|{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?
                );
                let vals: Vec<String> = (5..11)
                    .map(|i| Ok(norm(r.get::<_, V>(i)?)))
                    .collect::<rusqlite::Result<_>>()
                    .unwrap();
                Ok((key, vals))
            })
            .unwrap();
        for row in rows {
            let (k, v) = row.unwrap();
            ts_rows.insert(k, v);
        }
    }
    let mut ccs_rows: BTreeMap<String, Vec<String>> = BTreeMap::new();
    {
        let mut stmt = ccs
            .prepare(
                "SELECT date, provider_id, model, request_model, pricing_model, request_count,
                        input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
                        input_token_semantics
                 FROM usage_daily_rollups WHERE app_type = 'claude'",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                let key = format!(
                    "{}|{}|{}|{}|{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?
                );
                let vals: Vec<String> = (5..11)
                    .map(|i| Ok(norm(r.get::<_, V>(i)?)))
                    .collect::<rusqlite::Result<_>>()
                    .unwrap();
                Ok((key, vals))
            })
            .unwrap();
        for row in rows {
            let (k, v) = row.unwrap();
            ccs_rows.insert(k, v);
        }
    }
    let mut joined = 0u64;
    let mut field_mismatches = 0u64;
    let mut mismatches_by_field = [0u64; 6];
    let mut mismatch_examples: Vec<String> = Vec::new();
    for (key, ts_vals) in &ts_rows {
        let Some(ccs_vals) = ccs_rows.get(key) else {
            continue;
        };
        joined += 1;
        for (i, (a, b)) in ts_vals.iter().zip(ccs_vals.iter()).enumerate() {
            // TS 列序 input/output/cache_write/cache_read 对应 CCS
            // input/output/cache_creation/cache_read（写侧同源，逐位比较）。
            if a.trim() != b.trim() {
                field_mismatches += 1;
                mismatches_by_field[i] += 1;
                if mismatch_examples.len() < 10 {
                    mismatch_examples.push(format!("{key}#field{i}:{a}!={b}"));
                }
            }
        }
    }
    serde_json::json!({
        "ts_claude_rollup_rows": ts_rows.len(),
        "ccs_claude_rollup_rows": ccs_rows.len(),
        "joined": joined,
        "field_mismatches": field_mismatches,
        "mismatches_by_field": {
            "request_count": mismatches_by_field[0],
            "input": mismatches_by_field[1],
            "output": mismatches_by_field[2],
            "cache_write": mismatches_by_field[3],
            "cache_read": mismatches_by_field[4],
            "input_semantics": mismatches_by_field[5],
        },
        "missing_in_ccs": ts_rows.keys().filter(|k| !ccs_rows.contains_key(*k)).count(),
        "mismatch_examples": mismatch_examples,
        "fields": ["request_count", "input", "output", "cache_write", "cache_read", "input_semantics"],
        "note": "input_semantics 差异是写侧归一化：导入统一落库为 fresh(2)，与 CCS 来源的 0/1/2 原值不同属预期；四桶与请求数必须零差异",
    })
}

// ---------- 副本读数与清理 ----------

/// 读 TS 副本的 Claude 日汇总，按 `(day|model_identity)` 聚合（对比导入前后变化）。
fn read_ts_rollups(ts_copy: &Path) -> BTreeMap<String, RollupRowOut> {
    let conn = Connection::open_with_flags(ts_copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT day, model, request_count, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens
             FROM ccs_daily_usage WHERE app = 'claude-code'",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                u64_text(r, 3),
                u64_text(r, 4),
                u64_text(r, 5),
                u64_text(r, 6),
            ))
        })
        .unwrap();
    let mut out: BTreeMap<String, RollupRowOut> = BTreeMap::new();
    for row in rows {
        let (day, model, requests, input, output, cw, cr) = row.unwrap();
        let identity = ModelIdentity::parse(&model).identity_key();
        let agg = out
            .entry(format!("{day}|{identity}"))
            .or_insert_with(|| RollupRowOut {
                requests: 0,
                tokens: TokenCounts::default(),
            });
        agg.requests += requests.max(0) as u64;
        agg.tokens.input += input;
        agg.tokens.output += output;
        agg.tokens.cache_write += cw;
        agg.tokens.cache_read += cr;
    }
    out
}

#[derive(Clone, PartialEq)]
struct RollupRowOut {
    requests: u64,
    tokens: TokenCounts,
}

/// 删除本轮创建的数据库副本及其 WAL/SHM/迁移备份伴生文件。
fn remove_db_copy(path: &Path) {
    let _ = std::fs::remove_file(path);
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    for suffix in ["-wal", "-shm", "-backup-v4"] {
        let _ = std::fs::remove_file(path.with_file_name(format!("{name}{suffix}")));
    }
}
