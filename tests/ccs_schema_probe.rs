//! CCS 来源库结构核对（**只读、需显式运行、默认 ignore**）。
//!
//! 用途：cc-switch 的表结构或口径随版本变化时，用它重新核对导入侧假设的
//! 列名、唯一键、`input_token_semantics` 与 `data_source` 分布。
//!
//! 边界（与 `tests/real_data_probe.rs` 同一约定）：
//! - **只读**打开用户本机的 `~/.cc-switch/cc-switch.db`，不写入、不复制；
//! - 只打印**结构**与**聚合计数**（列名、唯一键、按应用/来源分组的行数），
//!   不打印任何提示词、代码或对话内容；模型名与 provider_id 属用量元数据，
//!   仅在需要确认口径时按需打印；
//! - 默认 `#[ignore]`：CI 与常规测试不运行，也不依赖用户机器上存在该库。
//!
//! 运行：`cargo test --offline --test ccs_schema_probe -- --ignored --nocapture`

use rusqlite::OpenFlags;

#[test]
#[ignore]
fn dump_ccs_schema_and_distributions() {
    let home = match std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        Ok(h) => h,
        Err(_) => {
            eprintln!("跳过：无法定位用户主目录");
            return;
        }
    };
    let path = std::path::PathBuf::from(home)
        .join(".cc-switch")
        .join("cc-switch.db");
    if !path.is_file() {
        eprintln!("跳过：本机没有 {}", path.display());
        return;
    }
    let conn = match rusqlite::Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("跳过：只读打开失败 {e}");
            return;
        }
    };

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap_or(-1);
    eprintln!("=== CCS user_version={user_version}");

    // 表与行数（只统计，不打印内容）。
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

    // 导入侧依赖的列必须存在。
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
        // 唯一键（导入侧按 request_id / 完整主键去重）。
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

    // 口径分布（只打印枚举值与计数，用于核对导入侧的映射与扣减规则）。
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
