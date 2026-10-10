//! CCS 来源库核对探针（**只读、需显式运行、默认 ignore**）。
//!
//! 用途：cc-switch 的表结构或口径随版本变化时，用它重新核对导入侧假设的列名、
//! 唯一键、`input_token_semantics` 与 `data_source` 分布；并在**真实数据**上跑
//! 一次端到端导入（写入隔离的临时历史库），确认分类守恒与重复导入幂等。
//!
//! 边界（与 `tests/real_data_probe.rs` 同一约定）：
//! - 只读打开用户本机的 `~/.cc-switch/cc-switch.db`，不写入、不复制；
//! - 只打印结构与聚合计数（列名、唯一键、按应用/来源分组的行数），不打印提示词、
//!   代码或对话内容；
//! - 默认 `#[ignore]`：CI 与常规测试不运行，也不依赖用户机器上存在该库。
//!
//! 运行：
//! `cargo test --offline --test ccs_schema_probe -- --ignored --nocapture`

use rusqlite::OpenFlags;

/// 只读打开本机 CCS 库；不可用（无库/无法打开）时返回 None 并打印跳过原因。
fn open_real_ccs() -> Option<rusqlite::Connection> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let path = std::path::PathBuf::from(home)
        .join(".cc-switch")
        .join("cc-switch.db");
    if !path.is_file() {
        eprintln!("跳过：本机没有 {}", path.display());
        return None;
    }
    match rusqlite::Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("跳过：只读打开失败 {e}");
            None
        }
    }
}

/// 结构与必需列核对。
#[test]
#[ignore]
fn dump_ccs_schema() {
    let Some(conn) = open_real_ccs() else {
        return;
    };
    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap_or(-1);
    eprintln!("=== CCS user_version={user_version}");

    let mut tables = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        for r in stmt.query_map([], |r| r.get::<_, String>(0)).unwrap() {
            tables.push(r.unwrap());
        }
    }
    for t in &tables {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| r.get(0))
            .unwrap_or(-1);
        eprintln!("--- table {t} rows={n}");
    }

    for (table, required) in [
        (
            "proxy_request_logs",
            [
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
            ]
            .as_slice(),
        ),
        (
            "usage_daily_rollups",
            [
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
            ]
            .as_slice(),
        ),
    ] {
        let mut cols = Vec::new();
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{table}\")"))
            .unwrap();
        for r in stmt.query_map([], |r| r.get::<_, String>(1)).unwrap() {
            cols.push(r.unwrap());
        }
        for col in required {
            eprintln!(
                "col[{table}.{col}] {}",
                if cols.iter().any(|c| c == col) {
                    "present"
                } else {
                    "MISSING（导入侧会拒绝该版本）"
                }
            );
        }
        let mut stmt = conn
            .prepare(&format!("PRAGMA index_list(\"{table}\")"))
            .unwrap();
        let idx: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for (name, unique) in idx {
            let mut inner = conn
                .prepare(&format!("PRAGMA index_info(\"{name}\")"))
                .unwrap();
            let cols: Vec<String> = inner
                .query_map([], |r| r.get::<_, Option<String>>(2))
                .unwrap()
                .map(|r| r.unwrap().unwrap_or_else(|| "<expr>".to_string()))
                .collect();
            eprintln!("unique[{table}] {name} unique={unique} cols={cols:?}");
        }
    }
}

/// 口径分布（只打印枚举值与计数，用于核对导入侧的映射与扣减规则）。
#[test]
#[ignore]
fn dump_ccs_distributions() {
    let Some(conn) = open_real_ccs() else {
        return;
    };
    for (label, sql) in [
        (
            "proxy: app_type x data_source x input_token_semantics",
            "SELECT app_type, COALESCE(data_source,'<null>'), input_token_semantics, COUNT(*)
             FROM proxy_request_logs GROUP BY 1,2,3 ORDER BY 1,2,3",
        ),
        (
            "rollup: app_type x input_token_semantics",
            "SELECT app_type, input_token_semantics, COUNT(*), SUM(request_count)
             FROM usage_daily_rollups GROUP BY 1,2 ORDER BY 1,2",
        ),
        (
            "proxy: distinct model/request_model/pricing_model",
            "SELECT COUNT(DISTINCT model), COUNT(DISTINCT request_model),
                    COUNT(DISTINCT pricing_model) FROM proxy_request_logs",
        ),
    ] {
        eprintln!("### {label}");
        let mut stmt = conn.prepare(sql).unwrap();
        let ncols = stmt.column_count();
        let rows = stmt
            .query_map([], |r| {
                let mut out: Vec<String> = Vec::new();
                for i in 0..ncols {
                    out.push(match r.get_ref(i).unwrap() {
                        rusqlite::types::ValueRef::Null => "NULL".to_string(),
                        rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                        rusqlite::types::ValueRef::Real(v) => format!("{v}"),
                        rusqlite::types::ValueRef::Text(t) => {
                            String::from_utf8_lossy(t).to_string()
                        }
                        rusqlite::types::ValueRef::Blob(_) => "<blob>".to_string(),
                    });
                }
                Ok(out.join(" | "))
            })
            .unwrap();
        for row in rows {
            eprintln!("    {}", row.unwrap());
        }
    }
}

/// 真实 CCS 库的端到端导入核对（**只读来源库 + 隔离历史库**，默认 ignore）。
///
/// 验证导入口径在真实数据上的自洽性：分类总数守恒、第一次有净新增、第二次
/// 完全幂等（不新增事件、不递增 generation）。只打印计数，不打印任何内容。
#[test]
#[ignore]
fn real_import_into_isolated_history_is_idempotent() {
    use tokenscope::history::{HistoryDb, RollupConflictPolicy};
    use tokenscope::import::ccs::{self, CcsSource};

    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"));
    let Ok(home) = home else {
        eprintln!("跳过：无法定位用户主目录");
        return;
    };
    let source_path = std::path::PathBuf::from(home)
        .join(".cc-switch")
        .join("cc-switch.db");
    if !source_path.is_file() {
        eprintln!("跳过：本机没有 {}", source_path.display());
        return;
    }
    let dir = std::env::temp_dir().join(format!("tokenscope-ccs-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let history = HistoryDb::open(&dir.join("history.db")).unwrap();
    let source = CcsSource::open(&source_path).unwrap();

    let first = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    let classified = first.requests_importable
        + first.requests_skipped_other_app
        + first.requests_skipped_duplicate_of_proxy
        + first.requests_rejected;
    assert_eq!(
        classified, first.requests_total,
        "分类总数必须与来源行数一致（不静默丢行）"
    );
    eprintln!(
        "真实 CCS 预览：明细 {} 条（可导入 {}、其他应用 {}、会话重复 {}、拒绝 {}），日汇总 {} 条（新增 {}、冲突 {}），净新增 token {}",
        first.requests_total,
        first.requests_importable,
        first.requests_skipped_other_app,
        first.requests_skipped_duplicate_of_proxy,
        first.requests_rejected,
        first.rollups_total,
        first.rollups_new,
        first.rollups_conflicting,
        first.net_new_tokens.total()
    );

    let report = ccs::commit(&first.plan_id, &history, RollupConflictPolicy::KeepExisting).unwrap();
    let events_after_first = history.event_count().unwrap();
    let generation_after_first = history.generation().unwrap();
    eprintln!(
        "真实 CCS 导入：批次 {}，新增 {}，更新 {}，已存在 {}，冲突 {}，日汇总落盘 {}",
        report.run_id,
        report.requests_inserted,
        report.requests_updated,
        report.requests_unchanged,
        report.requests_conflicted,
        report.rollups_snapshotted
    );
    assert_eq!(
        events_after_first,
        report.requests_inserted + report.requests_updated,
        "库内事件数必须等于本次新增 + 更新（不与已有历史重复）"
    );

    // 重复导入同一库：完全幂等（不新增事件、不递增 generation）。
    let second = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(second.would_insert, 0, "重复导入不得新增用量");
    let report2 = ccs::commit(
        &second.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
    )
    .unwrap();
    assert_eq!(report2.requests_inserted, 0);
    assert_eq!(history.event_count().unwrap(), events_after_first);
    assert_eq!(history.generation().unwrap(), generation_after_first);
    eprintln!("重复导入：新增 0、事件数不变、generation 不变（幂等）");

    let _ = std::fs::remove_dir_all(&dir);
}
