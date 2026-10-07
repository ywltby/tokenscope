//! RC01：查询身份**跨启动**唯一——旧进程的游标不得在新进程被接受。
//!
//! 2026-10-08 复核复现：`query_id` 由纯进程内计数派生（`q0-g0`），两个独立
//! 进程各自从 0 开始 → 第二进程建立"第一会话"时身份与第一进程完全相同，
//! 于是接受第一进程的旧游标，续到包含新增事件的新快照（total 4 → 5）。
//!
//! 本测试**真实启动子进程**（`current_exe()`），不是 `clear_query_registry`
//! 模拟重启——后者不重置计数器、不重建同名查询，抓不到该缺陷。
//!
//! 子进程通过 `--ignored --exact child_probe` 只跑探针；父进程检查两个
//! 子进程的退出码与产物。环境只传探针阶段与临时根目录，不继承真实来源
//! 配置；所有读写落在临时目录，不触碰真实 `~/.tokenscope` 与 agent 日志。

use std::path::{Path, PathBuf};
use std::process::Command;

use tokenscope::aggregate::GroupBy;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions};

const PHASE_ENV: &str = "TOKENSCOPE_QUERY_RESTART_PHASE";
const DIR_ENV: &str = "TOKENSCOPE_QUERY_RESTART_DIR";

/// 子进程探针：按阶段执行并写产物。正常套件里被 `#[ignore]` 跳过。
#[test]
#[ignore = "由 query_cursor_from_previous_process_is_rejected 作为子进程调用"]
fn child_probe() {
    let Ok(phase) = std::env::var(PHASE_ENV) else {
        return; // 正常套件（无阶段变量）直接跳过
    };
    let dir = PathBuf::from(std::env::var(DIR_ENV).expect("子进程必须收到临时根目录"));
    let result = match phase.as_str() {
        "a" => child_first_session(&dir),
        "b" => child_replay_old_cursor(&dir),
        other => Err(format!("未知探针阶段 {other}")),
    };
    if let Err(e) = result {
        // 子进程断言失败 → 非零退出，父进程据此判红。
        eprintln!("CHILD_FAILED: {e}");
        std::process::exit(3);
    }
}

/// 阶段 A：建立第一会话，把完整句柄与首页游标写到 a.json。
fn child_first_session(dir: &Path) -> Result<(), String> {
    let opts = opts(dir);
    let handle = query::begin_query_handle(&opts).map_err(|e| e.to_string())?;
    let page = query::query_events(
        &handle.query_id,
        &EventFilter {
            limit: Some(1),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let first = page.rows.first().ok_or("第一会话首页为空")?;
    let out = serde_json::json!({
        "query_id": handle.query_id,
        "cursor": first.cursor,
        "total": page.total,
        "first_input": first.input,
    });
    std::fs::write(dir.join("a.json"), out.to_string()).map_err(|e| e.to_string())
}

/// 阶段 B：以完全相同路径/筛选建立"第一会话"，提交 A 的旧游标。
/// 期望：新旧 ID 不同、旧游标明确拒绝、B 自己的游标正常分页。
fn child_replay_old_cursor(dir: &Path) -> Result<(), String> {
    let a: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("a.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let old_qid = a["query_id"]
        .as_str()
        .ok_or("a.json 缺 query_id")?
        .to_string();
    let old_cursor = a["cursor"].as_str().ok_or("a.json 缺 cursor")?.to_string();

    let opts = opts(dir);
    let handle = query::begin_query_handle(&opts).map_err(|e| e.to_string())?;
    let new_qid = handle.query_id.clone();

    // (1) 身份必须不同（这正是修复前失败的点：两个进程都得到 q0-g0）。
    if new_qid == old_qid {
        return Err(format!(
            "跨进程查询身份重复：新进程第一会话与旧进程相同 ({new_qid})"
        ));
    }

    // (2) 旧游标必须被显式拒绝。
    let err = query::query_events(
        &new_qid,
        &EventFilter {
            before: Some(old_cursor.clone()),
            ..Default::default()
        },
    )
    .err()
    .ok_or("旧进程游标被新会话接受（应拒绝）")?;
    let msg = err.to_string();
    if !msg.contains("query_expired") {
        return Err(format!("旧游标拒绝理由不是结构化过期错误: {msg}"));
    }

    // (3) 新会话自己的游标正常分页（拒绝旧身份不等于整体不可用）。
    let own = query::query_events(
        &new_qid,
        &EventFilter {
            limit: Some(1),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let own_cursor = own
        .rows
        .first()
        .map(|r| r.cursor.clone())
        .ok_or("新会话首页为空")?;
    let next = query::query_events(
        &new_qid,
        &EventFilter {
            limit: Some(1),
            before: Some(own_cursor),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    if next.rows.is_empty() {
        return Err("新会话自身游标无法续页".to_string());
    }

    let out = serde_json::json!({
        "new_query_id": new_qid,
        "old_query_id": old_qid,
        "old_rejected": true,
        "own_paging_ok": true,
        "total": own.total,
        "next_input": next.rows[0].input,
    });
    std::fs::write(dir.join("b.json"), out.to_string()).map_err(|e| e.to_string())
}

fn hermetic(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-query-restart-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 合成 codex 日志：每条 token_count 一个独立时间戳，输入量按给定顺序。
fn codex_body(inputs: &[u64]) -> String {
    let mut s = String::new();
    s.push_str(
        r#"{"timestamp":"2026-07-17T16:00:00.000Z","type":"session_meta","payload":{"id":"s1","session_id":"s1","cwd":"C:\\w\\a"}}"#,
    );
    s.push('\n');
    s.push_str(
        r#"{"timestamp":"2026-07-17T16:00:00.000Z","type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5.6-sol","cwd":"C:\\w\\a"}}"#,
    );
    s.push('\n');
    for (i, v) in inputs.iter().enumerate() {
        s.push_str(&format!(
            r#"{{"timestamp":"2026-07-17T16:00:{:02}.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{v},"output_tokens":0,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":{v}}}}}}}}}"#,
            i + 1
        ));
        s.push('\n');
    }
    s
}

fn write_log(dir: &Path, inputs: &[u64]) {
    let day = dir.join("codex").join("2026").join("07").join("17");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join("rollout-restart.jsonl"), codex_body(inputs)).unwrap();
}

fn opts(dir: &Path) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(dir.join("no-claude")),
        codex_dir: Some(dir.join("codex")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 只传阶段与临时根目录：清空继承环境（不泄漏真实 TOKENSCOPE_* / 来源
/// 配置），仅补回 Windows 运行必需的 SystemRoot/PATH/TEMP。
fn run_child(phase: &str, dir: &Path) -> std::process::Output {
    let exe = std::env::current_exe().expect("current_exe");
    let mut cmd = Command::new(exe);
    cmd.args([
        "--ignored",
        "--exact",
        "child_probe",
        "--test-threads=1",
        "--nocapture",
    ]);
    cmd.env_clear();
    for key in [
        "SystemRoot",
        "windir",
        "PATH",
        "TEMP",
        "TMP",
        "NUMBER_OF_PROCESSORS",
    ] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    cmd.env(PHASE_ENV, phase);
    cmd.env(DIR_ENV, dir);
    cmd.output().expect("spawn child probe")
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// 核心验收：旧进程游标在新进程被拒绝，新进程自身分页正常。
#[test]
fn query_cursor_from_previous_process_is_rejected() {
    let dir = hermetic("replay");
    // 第一进程看到 4 条事件；第一页（时间降序）应为最后一条 input=80。
    write_log(&dir, &[5, 10, 750, 80]);

    let a = run_child("a", &dir);
    assert!(
        a.status.success(),
        "子进程 A 失败：status={:?}\nstdout={}\nstderr={}",
        a.status.code(),
        String::from_utf8_lossy(&a.stdout),
        String::from_utf8_lossy(&a.stderr),
    );
    let a_json = read_json(&dir.join("a.json"));
    assert_eq!(a_json["total"], 4, "A 应看到 4 条事件");
    assert_eq!(a_json["first_input"], 80, "A 首页应为时间最新的一条");

    // 第一进程退出后追加一条合成事件（模拟日志增长）。
    write_log(&dir, &[5, 10, 750, 80, 12345]);

    let b = run_child("b", &dir);
    assert!(
        b.status.success(),
        "子进程 B 失败：status={:?}\nstdout={}\nstderr={}",
        b.status.code(),
        String::from_utf8_lossy(&b.stdout),
        String::from_utf8_lossy(&b.stderr),
    );
    let b_json = read_json(&dir.join("b.json"));
    assert_eq!(b_json["old_rejected"], true, "旧游标必须被拒绝");
    assert_eq!(b_json["own_paging_ok"], true, "新会话自身分页必须正常");
    assert_ne!(
        b_json["new_query_id"], b_json["old_query_id"],
        "跨进程查询身份必须不同"
    );
    assert_eq!(b_json["total"], 5, "新进程应看到追加后的事件");
    assert_eq!(
        b_json["next_input"], 80,
        "新会话第二页应是紧随其首页的 80（不是旧快照的行）"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 同进程内两个会话身份也不同（命名空间 + 序号），且旧游标互不可用。
/// 不固定匹配 query_id 的内部文本形式——只断言"不等 + 结构化拒绝"。
#[test]
fn same_process_sessions_have_distinct_identity() {
    let dir = hermetic("sameproc");
    write_log(&dir, &[5, 10, 750, 80]);
    let o = opts(&dir);

    let s1 = query::begin_query(&o).unwrap();
    let s2 = query::begin_query(&o).unwrap();
    assert_ne!(s1.query_id, s2.query_id, "同进程两次会话身份必须不同");

    let page = query::query_events(&s1.query_id, &EventFilter::default()).unwrap();
    let cursor = page.rows[0].cursor.clone();
    let err = query::query_events(
        &s2.query_id,
        &EventFilter {
            before: Some(cursor),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("query_expired"),
        "外会话游标必须结构化拒绝: {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 错误下钻指纹与 v1 旧游标同样被拒绝（回归，与快照契约测试同源）。
#[test]
fn wrong_drill_and_v1_cursor_are_rejected() {
    let dir = hermetic("drill");
    write_log(&dir, &[5, 10, 750, 80]);
    let o = opts(&dir);

    let snap = query::begin_query(&o).unwrap();
    let page = query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    let cursor = page.rows[0].cursor.clone();

    let err = query::query_events(
        &snap.query_id,
        &EventFilter {
            model: Some("gpt-5.6-sol".to_string()),
            before: Some(cursor),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("query_expired"), "{err}");

    let v1 = r#"{"ts":"2026-07-17T16:00:04Z","rid":"","seq":0}"#;
    let err = query::query_events(
        &snap.query_id,
        &EventFilter {
            before: Some(v1.to_string()),
            ..Default::default()
        },
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("query_expired") || msg.contains("游标格式非法"),
        "v1 游标必须显式拒绝: {msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
