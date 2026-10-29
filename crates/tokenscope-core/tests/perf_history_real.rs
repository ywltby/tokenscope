//! AC09 N13 真实规模性能（收口计划 §4「AC09 N13 真实规模性能（本地）」）。
//!
//! 默认 `#[ignore]`，不进常规套件与提交钩子；显式运行需 `TOKENSCOPE_REAL_PERF=1`：
//!
//! ```text
//! TOKENSCOPE_REAL_PERF=1 cargo test --release --test perf_history_real -- --ignored --nocapture
//! ```
//!
//! **阶段 1（当前提交）：只用合成来源验证测试架本身**——冷/热采集、首页汇总
//! 与明细前两页可测量；冷热请求数与四桶一致；默认根与重叠追加根不重复计数；
//! 隔离不变量成立。真实 `~/.claude`、`~/.codex` 来源的读取属 B4 后续阶段
//! （N13 保持开放），本文件不含任何真实来源路径；旧 `perf_real_data.rs`
//! 不改不删（它是 CLAUDE.md 记载的真实缓存预热工具），N13 的结论只取自
//! 本文件，不引用旧测试的数字。
//!
//! 隔离不变量（AC09 要求，全部显式注入）：
//! - 历史库 `history_path`、缓存 `cache_dir`、价格索引 `pricing_index`、
//!   价格文件 `pricing_path` 与证据输出全部落在 hermetic 临时目录；
//! - OpenRouter/models.dev 快照注入不存在的路径（不读、不写真实快照）；
//! - 来源计划 `source_plans` 显式注入合成根：`Some` 完全取代 `claude_dir`/
//!   `codex_dir` 的默认根语义，默认根（`~/.claude` 等）不会被咨询；
//!   `claude_mapping: None` 不读真实 `~/.claude.json`；其余 dir 类字段
//!   全部指向隔离目录内的不存在路径，即使未来回归到 dir 语义也不会
//!   意外落在真实用户目录（测试内另断言全部来源根位于隔离目录内）。
//!
//! 证据边界（复核收窄，如实标注——观察范围之外的部分为**未证明**）：
//! - 写侧：运行前后对真实 `~/.tokenscope` 取指纹且必须不变（本机无该
//!   目录则必须始终不存在，本测试绝不新建它）。指纹只比较**文件数与
//!   总字节数**——同长度的内容改写不可检测（敏感性用例
//!   `ac09_fingerprint_scope_and_blind_spot` 已在合成目录钉住该边界）；
//!   「逐字节零写入」未证明。
//! - 读侧：请求数精确等于合成规模、四桶与期望公式吻合、按项目分组恰
//!   为合成项目集合——只能排除**产生有效事件的额外来源**；空目录或
//!   零有效事件来源的读取不可由结果排除，结构性保证是 `source_plans`
//!   显式注入（见上）。本测试读取真实 `~/.tokenscope` 的**目录元数据**
//!   （文件名与大小，递归），不读文件内容，不输出文件名清单。
//! - 分页：前两页与合成数据的**独立期望顺序逐位核对**（不重复也不
//!   漏行），并以「跳过中间一段」的反例证明核对对漏行有区分力。
//!
//! 测量（只记录，不设硬门槛）：各阶段耗时（细分阶段由 collect_all 的 INFO
//! 日志给出，--nocapture 可见）、事件与文件数量（来源统计）、Rust 堆峰值
//! （N05 计数分配器；不覆盖 SQLite C 分配与完整 RSS，见
//! docs/stats-semantics.md §3.5）、进程峰值工作集（Windows 为进程记录的
//! 峰值经外部 PowerShell 查询，无采样间隙；不可获得时如实标注）。
//! 证据 JSON 写入隔离目录（不入库）；本测试不输出任何日志正文内容。

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Instant;

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::query;
use tokenscope::report::{EventFilter, SourcePlan, SummaryOptions};

// ── N05 计数分配器（只测 Rust 堆；SQLite C 分配与完整 RSS 不在内） ─────

struct TrackingAlloc;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let prev = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            PEAK_BYTES.fetch_max(prev.saturating_add(layout.size()), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // 计数只回退、绝不下溢 panic；真实释放仍完全交给系统分配器。
        let size = layout.size();
        let _ = LIVE_BYTES.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            Some(v.saturating_sub(size))
        });
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[global_allocator]
static TRACKING_ALLOC: TrackingAlloc = TrackingAlloc;

/// 两个用例串行执行：计时与堆峰值都是进程级观测量，不能并行互扰。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn reset_peak() {
    PEAK_BYTES.store(0, Ordering::Relaxed);
}

fn peak_bytes() -> usize {
    PEAK_BYTES.load(Ordering::Relaxed)
}

// ── 门禁、隔离目录与真实目录指纹 ────────────────────────────────────

/// 明细默认页大小（`EventFilter::limit = None` 时产品口径为 200）。
const PAGE: usize = 200;

fn gate_enabled() -> bool {
    std::env::var("TOKENSCOPE_REAL_PERF").ok().as_deref() == Some("1")
}

static SEQ: AtomicU32 = AtomicU32::new(0);

fn hermetic_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-perf-history-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 目录指纹：(文件数, 总字节数)。目录不存在 = `None`。
/// 观察范围：只能检测文件数与字节数变化；**同长度的内容改写不可检测**
/// （敏感性用例 `ac09_fingerprint_scope_and_blind_spot` 在合成目录上
/// 钉住该边界）。递归只读目录元数据（文件名与大小），不读文件内容。
fn fingerprint_dir(dir: &Path) -> Option<(u64, u64)> {
    if !dir.exists() {
        return None;
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                files += 1;
                bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    Some((files, bytes))
}

/// 真实 `~/.tokenscope` 指纹：运行前后必须不变（观察范围见
/// [`fingerprint_dir`]；本测试绝不新建用户数据目录）。
fn real_tokenscope_fingerprint() -> Option<(u64, u64)> {
    fingerprint_dir(&tokenscope::report::data_dir().ok()?)
}

/// 进程峰值工作集：Windows 为进程记录的峰值（GetProcessMemoryInfo，无采样
/// 间隙）经外部 PowerShell 查询，作为参考值；不可获得时 `None` 并如实标注。
fn process_peak_working_set_bytes() -> Option<u64> {
    if !cfg!(windows) {
        return None;
    }
    let pid = std::process::id();
    for prop in ["PeakWorkingSet64", "PeakWorkingSet"] {
        let script = format!("(Get-Process -Id {pid}).{prop}");
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            && out.status.success()
            && let Ok(v) = String::from_utf8_lossy(&out.stdout).trim().parse::<u64>()
        {
            return Some(v);
        }
    }
    None
}

// ── 合成来源（阶段 1：唯一来源；B4 后续阶段才换成只读真实来源） ────────

/// 数据规模：Claude 6 项目 × 20 文件 × 350 事件 = 42 000 条，
/// Codex 20 文件 × 500 事件 = 10 000 条，合计 52 000 条请求、140 个文件；
/// 时间跨多个自然日（Day 粒度聚合非平凡）。事件时间戳全局唯一且按
/// 全局序号单调递增，保证明细分页游标（ts|record_id）确定。
const CLAUDE_SLUGS: usize = 6;
const CLAUDE_FILES: usize = 20;
const CLAUDE_EVENTS: usize = 350;
const CODEX_FILES: usize = 20;
const CODEX_EVENTS: usize = 500;
const CLAUDE_TOTAL: u64 = (CLAUDE_SLUGS * CLAUDE_FILES * CLAUDE_EVENTS) as u64;
const CODEX_TOTAL: u64 = (CODEX_FILES * CODEX_EVENTS) as u64;
const TOTAL_EVENTS: u64 = CLAUDE_TOTAL + CODEX_TOTAL;
const TOTAL_FILES: u64 = (CLAUDE_SLUGS * CLAUDE_FILES + CODEX_FILES) as u64;

/// Claude 事件四桶（event = 文件内序号）：随序号变化，使四桶非平凡。
fn claude_usage(event: usize) -> (u64, u64, u64, u64) {
    (
        100 + (event % 50) as u64,
        40 + (event % 20) as u64,
        5 + (event % 5) as u64,
        10 + (event % 10) as u64,
    )
}

/// Codex 事件三桶（无写入档）：input / output / cache_read。
/// 两条 fixture 约束：① R02 口径 `total = input + output` 且 `cached ≤ input`
/// （cached 是 input 的子集），否则整行按坏行计、事件被丢弃；
/// ② input 逐事件唯一——Codex 去重键是 (session, model, 四桶)，会话内
/// 相同用量会被折叠为一条（重复额度报告语义），周期性用量会丢事件。
fn codex_usage(event: usize) -> (u64, u64, u64) {
    (
        100 + event as u64,
        10 + (event % 9) as u64,
        ((event % 3) + 1) as u64 * 25,
    )
}

/// Claude 四桶期望合计（与 fixture 公式同源，供冷/热一致之外的第二重断言）。
fn claude_expected(slugs: usize, files: usize, events: usize) -> (u64, u64, u64, u64) {
    let mut inp = 0u64;
    let mut out = 0u64;
    let mut cw = 0u64;
    let mut cr = 0u64;
    for _ in 0..slugs * files {
        for e in 0..events {
            let (i, o, w, r) = claude_usage(e);
            inp += i;
            out += o;
            cw += w;
            cr += r;
        }
    }
    (inp, out, cw, cr)
}

/// 全部来源（Claude + Codex）四桶期望合计。
fn expected_buckets() -> (u64, u64, u64, u64) {
    let (mut inp, mut out, cw, mut cr) = claude_expected(CLAUDE_SLUGS, CLAUDE_FILES, CLAUDE_EVENTS);
    for _ in 0..CODEX_FILES {
        for e in 0..CODEX_EVENTS {
            let (i, o, r) = codex_usage(e);
            // Codex 守恒口径：reported input 含子集 cached，事件存净输入
            // （input − cached − cache_write），与 codex.rs 事件构造一致。
            inp += i - r;
            out += o;
            cr += r;
        }
    }
    (inp, out, cw, cr)
}

/// 合成事件的展示时间（明细行口径：解析时区 "YYYY-MM-DD HH:MM:SS"，
/// 与 report 的 `%F %T` 一致）。固定时区下展示串字典序 = 时间序。
fn display_ts(secs: i64, tz: &jiff::tz::TimeZone) -> String {
    jiff::Timestamp::from_second(secs)
        .unwrap()
        .to_zoned(tz.clone())
        .strftime("%F %T")
        .to_string()
}

/// 全部合成事件的**独立期望顺序**：(展示 ts, record_id) 按 UTC 时间降序。
/// 独立于历史库、快照与游标机制——排序直接来自 fixture 公式。
fn expected_rows(tz: &jiff::tz::TimeZone) -> Vec<(String, String)> {
    let mut rows: Vec<(i64, String)> = Vec::with_capacity(TOTAL_EVENTS as usize);
    for s in 0..CLAUDE_SLUGS {
        for f in 0..CLAUDE_FILES {
            for e in 0..CLAUDE_EVENTS {
                let g = ((s * CLAUDE_FILES + f) * CLAUDE_EVENTS + e) as i64;
                rows.push((CLAUDE_BASE + g * 10, format!("msg-{s}-{f}-{e}")));
            }
        }
    }
    for k in 0..CODEX_FILES {
        for e in 0..CODEX_EVENTS {
            let idx = (k * CODEX_EVENTS + e) as i64;
            // Codex 行 record_id 为空（时间戳即请求身份）。
            rows.push((CODEX_BASE + idx * 5, String::new()));
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    rows.into_iter()
        .map(|(ts, rid)| (display_ts(ts, tz), rid))
        .collect()
}

/// 逐位核对：`rows` 必须与期望顺序从 `offset`（0 起）起完全一致——
/// 既能发现重复，也能发现漏行（旧「两页游标不重复」检查发现不了漏行）。
fn rows_match_expected(
    rows: &[tokenscope::report::EventRow],
    expected: &[(String, String)],
    offset: usize,
) -> bool {
    rows.len() <= expected.len() - offset
        && rows
            .iter()
            .zip(expected[offset..].iter())
            .all(|(r, (ts, rid))| r.ts == *ts && r.record_id == *rid)
}

fn ts_at(base_secs: i64, offset_secs: i64) -> String {
    jiff::Timestamp::from_second(base_secs + offset_secs)
        .unwrap()
        .to_string()
}

const CLAUDE_BASE: i64 = 1783728000; // 2026-07-11T00:00:00Z（步长 10 s，≡ 0 mod 10）
// Codex 起点 +4 s、步长 5 s → 网格 ≡ 4/9 mod 10，与 Claude 网格互斥：
// 全部 ts 全局唯一，期望顺序无需建模 (ts, record_id) 平局规则；
// 20 文件 × 500 事件 × 5 s ≈ 13.9 h，全部落在 2026-07-21 当日。
const CODEX_BASE: i64 = 1784613604; // 2026-07-21T06:00:04Z

fn write_claude_tree(root: &Path, slugs: usize, files: usize, events: usize) {
    for slug in 0..slugs {
        let slug_dir = root.join(format!("perf-project-{slug}"));
        std::fs::create_dir_all(&slug_dir).unwrap();
        for file in 0..files {
            let mut body = String::new();
            for event in 0..events {
                let g = (slug * files + file) * events + event;
                let (input, output, cache_creation, cache_read) = claude_usage(event);
                let line = serde_json::json!({
                    "type": "assistant",
                    "timestamp": ts_at(CLAUDE_BASE, g as i64 * 10),
                    "sessionId": format!("perf-c-{slug}-{file}"),
                    "cwd": format!("C:/perf/work{slug}"),
                    "message": {
                        "id": format!("msg-{slug}-{file}-{event}"),
                        "model": "claude-sonnet-4-5",
                        "usage": {
                            "input_tokens": input,
                            "output_tokens": output,
                            "cache_creation_input_tokens": cache_creation,
                            "cache_read_input_tokens": cache_read,
                        }
                    }
                });
                body.push_str(&line.to_string());
                body.push('\n');
            }
            std::fs::write(slug_dir.join(format!("session-{file}.jsonl")), body).unwrap();
        }
    }
}

fn write_codex_tree(root: &Path) {
    let day = root.join("2026").join("07").join("21");
    std::fs::create_dir_all(&day).unwrap();
    for file in 0..CODEX_FILES {
        let sid = format!("perf-x-{file}");
        let cwd = format!("C:/perf/x{file}");
        let base = CODEX_BASE + (file * CODEX_EVENTS) as i64 * 5;
        let mut body = String::new();
        let meta = serde_json::json!({
            "timestamp": ts_at(base, 0),
            "type": "session_meta",
            "payload": {"id": sid, "session_id": sid, "cwd": cwd}
        });
        body.push_str(&meta.to_string());
        body.push('\n');
        let turn = serde_json::json!({
            "timestamp": ts_at(base, 0),
            "type": "turn_context",
            "payload": {"turn_id": format!("t-{file}"), "model": "gpt-5.6", "cwd": cwd}
        });
        body.push_str(&turn.to_string());
        body.push('\n');
        for event in 0..CODEX_EVENTS {
            let (input, output, cached) = codex_usage(event);
            let line = serde_json::json!({
                "timestamp": ts_at(base, event as i64 * 5),
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {"last_token_usage": {
                        "input_tokens": input,
                        "output_tokens": output,
                        "cached_input_tokens": cached,
                        "cache_write_input_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": input + output,
                    }}
                }
            });
            body.push_str(&line.to_string());
            body.push('\n');
        }
        std::fs::write(day.join(format!("rollout-{file}.jsonl")), body).unwrap();
    }
}

/// 全注入的汇总选项：所有存储路径与来源计划都显式给出，绝不依赖默认根。
/// `source_plans` 为 `Some` 时完全取代 dir 字段语义；dir 字段仍指向隔离
/// 目录内的不存在路径（双保险，见模块注释）。
fn build_opts(dir: &Path, plans: Vec<SourcePlan>) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_enabled: Some(true),
        codex_enabled: Some(true),
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        claude_dir: Some(dir.join("unused-claude")),
        codex_dir: Some(dir.join("unused-codex")),
        claude_projects_path: Some(dir.join("unused-claude.json")),
        cache_dir: Some(dir.join("cache")),
        history_path: Some(dir.join("history.db")),
        pricing_path: Some(dir.join("pricing.toml")),
        pricing_index: Some(dir.join("pricing-index.json")),
        openrouter_path: Some(dir.join("no-such-openrouter.json")),
        modelsdev_path: Some(dir.join("no-such-modelsdev.json")),
        tz: Some("Asia/Shanghai".to_string()),
        source_plans: Some(plans),
        ..Default::default()
    }
}

fn write_pricing(dir: &Path) {
    std::fs::write(
        dir.join("pricing.toml"),
        r#"
[[model]]
prefix = "claude-sonnet-4-5"
input = 3.0
output = 15.0
cache_write = 3.75
cache_read = 0.3

[[model]]
prefix = "gpt-5.6"
input = 1.25
output = 10.0
cache_write = 0.0
cache_read = 0.1
"#,
    )
    .unwrap();
}

/// 阶段计时结果（毫秒）。
#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
struct PhaseMs {
    begin: u128,
    summary: u128,
    page1: u128,
    page2: u128,
}

/// 一轮「begin_query（含采集）→ 首页汇总 → 明细前两页」，全部计时并断言。
/// 分页核对 = 旧口径「两页不重复」+ 与独立期望顺序逐位一致（不重不漏）；
/// 另取跳过中间一段的第三页作反例，证明逐位核对对漏行有区分力。
fn run_round(
    label: &str,
    opts: &SummaryOptions,
    expected: &[(String, String)],
) -> (tokenscope::report::SummaryReport, PhaseMs) {
    let t = Instant::now();
    let snap =
        query::begin_query_with(opts, true).unwrap_or_else(|e| panic!("{label} 采集失败: {e}"));
    let mut ph = PhaseMs {
        begin: t.elapsed().as_millis(),
        ..Default::default()
    };

    let t = Instant::now();
    let sum =
        query::query_summary(&snap.query_id).unwrap_or_else(|e| panic!("{label} 汇总失败: {e}"));
    ph.summary = t.elapsed().as_millis();

    let t = Instant::now();
    let page1 = query::query_events(&snap.query_id, &EventFilter::default())
        .unwrap_or_else(|e| panic!("{label} 明细第一页失败: {e}"));
    ph.page1 = t.elapsed().as_millis();
    assert_eq!(page1.rows.len(), PAGE, "{label}：第一页应为满页 {PAGE}");
    assert_eq!(page1.total, TOTAL_EVENTS, "{label}：明细总数应等于合成规模");

    let t = Instant::now();
    let cursor = page1.rows[PAGE - 1].cursor.clone();
    let page2 = query::query_events(
        &snap.query_id,
        &EventFilter {
            before: Some(cursor),
            ..EventFilter::default()
        },
    )
    .unwrap_or_else(|e| panic!("{label} 明细第二页失败: {e}"));
    ph.page2 = t.elapsed().as_millis();
    assert_eq!(page2.rows.len(), PAGE, "{label}：第二页应为满页 {PAGE}");
    // 旧口径：两页游标集合不相交——只能证明不重复，不能证明不漏行。
    let seen: std::collections::HashSet<&str> =
        page1.rows.iter().map(|r| r.cursor.as_str()).collect();
    assert!(
        page2.rows.iter().all(|r| !seen.contains(r.cursor.as_str())),
        "{label}：翻页不得重复"
    );
    // 期望顺序逐位核对：前两页必须恰为期望的第 1–400 位（不重复也不漏行）。
    assert!(
        rows_match_expected(&page1.rows, expected, 0)
            && rows_match_expected(&page2.rows, expected, PAGE),
        "{label}：前两页必须与合成数据的独立期望顺序逐位一致"
    );

    // 跳段反例：以正确第二页末行为游标取下一页（期望第 401–600 位）。
    // 若把它误当第二页，与第一页无交集——旧「不重复」检查放行；
    // 逐位核对必须在偏移 PAGE 处拒绝（检出漏行），并在偏移 2*PAGE 处
    // 接受（正控制：核对函数本身对正确位置不放空）。
    let skipped = query::query_events(
        &snap.query_id,
        &EventFilter {
            before: Some(page2.rows[PAGE - 1].cursor.clone()),
            ..EventFilter::default()
        },
    )
    .unwrap_or_else(|e| panic!("{label} 跳段反例页失败: {e}"));
    assert_eq!(skipped.rows.len(), PAGE, "{label}：反例页应为满页 {PAGE}");
    assert!(
        skipped
            .rows
            .iter()
            .all(|r| !seen.contains(r.cursor.as_str())),
        "{label}：反例页必须与第一页不重复（构成有效反例的前提）"
    );
    assert!(
        !rows_match_expected(&skipped.rows, expected, PAGE),
        "{label}：逐位核对必须拒绝「跳过中间一段」的第二页"
    );
    assert!(
        rows_match_expected(&skipped.rows, expected, 2 * PAGE),
        "{label}：反例页应为期望顺序的第 {}–{} 位（正控制）",
        2 * PAGE + 1,
        3 * PAGE
    );

    println!(
        "{label}：begin（含采集）{} ms / 汇总 {} ms / 明细第1页 {} ms / 第2页 {} ms",
        ph.begin, ph.summary, ph.page1, ph.page2
    );
    (sum, ph)
}

/// 四桶 + 请求数 + 费用的等值断言（AC09 一致性口径）。
fn assert_totals_equal(
    label: &str,
    a: &tokenscope::aggregate::Group,
    b: &tokenscope::aggregate::Group,
) {
    assert_eq!(a.requests, b.requests, "{label}：请求数不一致");
    assert_eq!(a.tokens.input, b.tokens.input, "{label}：input 不一致");
    assert_eq!(a.tokens.output, b.tokens.output, "{label}：output 不一致");
    assert_eq!(
        a.tokens.cache_write, b.tokens.cache_write,
        "{label}：cache_write 不一致"
    );
    assert_eq!(
        a.tokens.cache_read, b.tokens.cache_read,
        "{label}：cache_read 不一致"
    );
    assert!(
        (a.cost_usd - b.cost_usd).abs() < 1e-9,
        "{label}：费用不一致 {} vs {}",
        a.cost_usd,
        b.cost_usd
    );
}

/// 阶段 1 主流程：合成来源冷/热两轮 + 一致性 + 隔离验证 + 测量记录。
#[test]
#[ignore = "AC09 N13 性能测试架：需显式设置 TOKENSCOPE_REAL_PERF=1（当前为合成来源阶段）"]
fn ac09_synthetic_cold_warm_pipeline() {
    let _g = serial();
    if !gate_enabled() {
        eprintln!("跳过：未设置 TOKENSCOPE_REAL_PERF=1");
        return;
    }
    // 阶段日志打到 stdout（--nocapture 可见）；不装文件 appender。
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_ansi(false)
        .try_init();
    reset_peak();

    let dir = hermetic_dir("main");
    let claude_root = dir.join("claude").join("projects");
    let codex_root = dir.join("codex").join("sessions");
    write_claude_tree(&claude_root, CLAUDE_SLUGS, CLAUDE_FILES, CLAUDE_EVENTS);
    write_codex_tree(&codex_root);
    write_pricing(&dir);

    // 隔离基线：真实数据目录运行前指纹（不存在 = None）。
    let fp_before = real_tokenscope_fingerprint();

    let plans = vec![
        SourcePlan {
            agent: AgentKind::ClaudeCode,
            root: claude_root.clone(),
            claude_mapping: None,
        },
        SourcePlan {
            agent: AgentKind::Codex,
            root: codex_root.clone(),
            claude_mapping: None,
        },
    ];
    // 读侧的结构性保证（可断言的部分）：全部来源根必须位于隔离目录内。
    assert!(
        plans.iter().all(|p| p.root.starts_with(&dir)),
        "来源根必须全部位于隔离目录内"
    );
    let opts = build_opts(&dir, plans.clone());

    // 独立期望顺序（来自 fixture 公式，与库/游标机制无关）。
    let tz = jiff::tz::TimeZone::get("Asia/Shanghai").expect("仓库默认时区常量且合法");
    let expected = expected_rows(&tz);

    // ── 冷采集（空历史库 → 全量解析入库）+ 首页汇总 + 明细前两页
    let (cold, cold_ph) = run_round("冷轮", &opts, &expected);

    // ── 热采集（第二轮无新数据：来源指纹未变，不重解析）+ 同口径查询
    let (warm, warm_ph) = run_round("热轮", &opts, &expected);

    // ── 读侧有区分度检查：按项目分组恰为合成项目集合（6 Claude + 20
    // Codex = 26），请求数合计等于合成规模——产生有效事件的额外来源
    // 必然新增分组或超出合计（空来源/零有效事件来源不可由此排除）。
    let mut proj_opts = build_opts(&dir, plans);
    proj_opts.by = GroupBy::Project;
    let proj_snap = query::begin_query_with(&proj_opts, true).expect("项目分组采集失败");
    let proj = query::query_summary(&proj_snap.query_id).expect("项目分组读取失败");
    // 注：聚合器总是把「合计」行追加进 groups（aggregate.rs），先剔除再核对。
    let (real_groups, totals_row) = match proj.groups.split_last() {
        Some((last, rest)) if last.key == "合计" => (rest, Some(last)),
        _ => (proj.groups.as_slice(), None),
    };
    assert_eq!(
        totals_row.map(|g| g.requests),
        Some(TOTAL_EVENTS),
        "合计行请求数应等于合成规模"
    );
    assert_eq!(
        real_groups.len(),
        CLAUDE_SLUGS + CODEX_FILES,
        "项目分组数应等于合成项目数（6 Claude + 20 Codex）"
    );
    assert!(
        real_groups.iter().all(|g| g.requests > 0),
        "每个合成项目都应有请求"
    );
    let proj_requests: u64 = real_groups.iter().map(|g| g.requests).sum();
    assert_eq!(
        proj_requests, TOTAL_EVENTS,
        "项目分组请求数合计应等于合成规模"
    );

    // ── 一致性（AC09）：冷、热两轮的请求数与四桶完全一致
    assert_totals_equal("冷/热", &cold.totals, &warm.totals);
    let (ein, eout, ecw, ecr) = expected_buckets();
    assert_eq!(
        cold.totals.requests, TOTAL_EVENTS,
        "请求数应精确等于合成规模"
    );
    assert_eq!(
        cold.totals.tokens.input, ein,
        "input 合计应等于 fixture 期望"
    );
    assert_eq!(
        cold.totals.tokens.output, eout,
        "output 合计应等于 fixture 期望"
    );
    assert_eq!(
        cold.totals.tokens.cache_write, ecw,
        "cache_write 合计应等于 fixture 期望"
    );
    assert_eq!(
        cold.totals.tokens.cache_read, ecr,
        "cache_read 合计应等于 fixture 期望"
    );
    assert!(cold.totals.cost_usd > 0.0, "两个模型都有价格，费用应为正");

    // ── 来源统计（事件与文件数量；events 为历史库内该应用事件总数）
    let mut source_rows = Vec::new();
    let mut files_seen = 0u64;
    for s in &warm.sources {
        files_seen += s.stats.files_scanned;
        source_rows.push(serde_json::json!({
            "agent": s.agent.as_str(),
            "files_scanned": s.stats.files_scanned,
            "events_in_history": s.stats.events,
        }));
    }
    assert_eq!(files_seen, TOTAL_FILES, "发现的文件数应等于合成规模");

    // ── 隔离验证：写入只落注入路径，真实数据目录指纹不变
    assert!(
        dir.join("history.db").is_file(),
        "历史库必须落在注入的 history_path"
    );
    assert!(
        dir.join("pricing-index.json").is_file(),
        "价格索引必须落在注入的 pricing_index"
    );
    // H04 起生产采集不再创建解析缓存 cache.db（历史库是唯一事实源，
    // 遗留 cache.db 只作迁移来源读取），因此此处不存在 cache 产物。
    let fp_after = real_tokenscope_fingerprint();
    assert_eq!(
        fp_before, fp_after,
        "真实 ~/.tokenscope 指纹变化（{fp_before:?} → {fp_after:?}）：\
         文件数或总字节数被改变（观察范围 = 文件数 + 总字节数，见模块注释）。\
         若确有 TokenScope GUI 同时运行，请先关闭后重跑再归因"
    );
    // 证据边界（如实标注）：指纹只证明文件数与总字节数不变（同长度内容
    // 改写不可检测）；请求数与项目分组只排除「产生有效事件的额外来源」。
    // 逐字节零写入与空来源零读取未证明——结构性保证是 source_plans 显式
    // 注入（见上方来源根断言）。

    // ── 峰值测量（覆盖整个冷/热流程；fixture 写入的少量分配可忽略）
    let heap_peak = peak_bytes();
    let working_set_peak = process_peak_working_set_bytes();

    // ── 证据 JSON（写入隔离目录，不入库；不含任何日志正文）
    let evidence = serde_json::json!({
        "phase": "AC09 阶段1（合成来源，N13 开放）",
        "scale": {
            "claude_files": CLAUDE_SLUGS * CLAUDE_FILES,
            "claude_events": CLAUDE_TOTAL,
            "codex_files": CODEX_FILES,
            "codex_events": CODEX_TOTAL,
            "total_requests": TOTAL_EVENTS,
        },
        "cold_ms": cold_ph,
        "hot_ms": warm_ph,
        "totals": {
            "requests": cold.totals.requests,
            "input": cold.totals.tokens.input,
            "output": cold.totals.tokens.output,
            "cache_write": cold.totals.tokens.cache_write,
            "cache_read": cold.totals.tokens.cache_read,
            "cost_usd": cold.totals.cost_usd,
        },
        "sources": source_rows,
        "rust_heap_peak_bytes": heap_peak,
        "process_peak_working_set_bytes": working_set_peak,
        "isolation": {
            "history_path": dir.join("history.db"),
            "cache_dir": dir.join("cache"),
            "pricing_index": dir.join("pricing-index.json"),
            "real_tokenscope_present_before": fp_before.is_some(),
            "real_tokenscope_fingerprint_unchanged": true,
            "fingerprint_scope": "files_and_bytes_only（同长度内容改写不可检测）",
        },
        "verification": {
            "page_rows_checked": 2 * PAGE,
            "expected_order_match": true,
            "skip_counterexample_rejected": true,
            "project_groups": real_groups.len(),
            "project_requests_sum": proj_requests,
        },
    });
    let evidence_path = dir.join("ac09-stage1-evidence.json");
    std::fs::write(
        &evidence_path,
        serde_json::to_string_pretty(&evidence).unwrap(),
    )
    .unwrap();

    println!(
        "规模：{} 文件 / {} 请求；冷轮 begin {} ms，热轮 begin {} ms；热轮汇总 {} ms / 两页明细 {} + {} ms",
        TOTAL_FILES,
        TOTAL_EVENTS,
        cold_ph.begin,
        warm_ph.begin,
        warm_ph.summary,
        warm_ph.page1,
        warm_ph.page2
    );
    println!(
        "四桶合计：input {} / output {} / cache_write {} / cache_read {}；费用 ${:.4}",
        cold.totals.tokens.input,
        cold.totals.tokens.output,
        cold.totals.tokens.cache_write,
        cold.totals.tokens.cache_read,
        cold.totals.cost_usd
    );
    println!(
        "Rust 堆峰值 {} MiB（不含 SQLite C 分配）；进程峰值工作集 {}",
        heap_peak / (1024 * 1024),
        match working_set_peak {
            Some(v) => format!("{} MiB（参考值）", v / (1024 * 1024)),
            None => "不可获得（如实标注）".to_string(),
        }
    );
    println!("证据 JSON：{}", evidence_path.display());
}

// ── 多根重叠：默认根 + 与之重叠的追加根不重复计数（AC09 一致性） ───────

#[cfg(windows)]
fn make_alias(link: &Path, target: &Path) {
    let _ = std::fs::remove_file(link);
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("启动 mklink");
    assert!(
        out.status.success(),
        "创建 junction 失败（本环境必须支持，skip 不算通过）: {:?} / {:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(not(windows))]
fn make_alias(link: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, link).expect("创建符号链接失败");
}

/// 默认根加一个与默认根重叠的追加根（同一物理目录的别名）：结果与单根
/// 一致（发现层按规范化身份认领，同应用首者得，文件只解析一次），
/// 耗时如实记录（承接 PR3 §4.4）。
#[test]
#[ignore = "AC09 N13 性能测试架：需显式设置 TOKENSCOPE_REAL_PERF=1（当前为合成来源阶段）"]
fn ac09_default_root_plus_overlapping_alias_counts_once() {
    let _g = serial();
    if !gate_enabled() {
        eprintln!("跳过：未设置 TOKENSCOPE_REAL_PERF=1");
        return;
    }
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_ansi(false)
        .try_init();
    reset_peak();

    let dir = hermetic_dir("overlap");
    let root = dir.join("claude").join("projects");
    const SLUGS: usize = 2;
    const FILES: usize = 3;
    const EVENTS: usize = 80;
    const TOTAL: u64 = (SLUGS * FILES * EVENTS) as u64;
    write_claude_tree(&root, SLUGS, FILES, EVENTS);
    write_pricing(&dir);

    let alias = dir.join("claude-alias");
    make_alias(&alias, &root);

    let fp_before = real_tokenscope_fingerprint();

    // 单根基线：独立历史库/缓存，只注入默认根（读同一棵物理树）。
    let baseline_dir = hermetic_dir("overlap-baseline");
    write_pricing(&baseline_dir);
    let base_opts = build_opts(
        &baseline_dir,
        vec![SourcePlan {
            agent: AgentKind::ClaudeCode,
            root: root.clone(),
            claude_mapping: None,
        }],
    );
    let base_snap = query::begin_query(&base_opts).expect("单根基线采集失败");
    let base = query::query_summary(&base_snap.query_id).expect("单根基线汇总失败");

    // 双根：默认根 + 重叠追加根（别名 junction），独立历史库/缓存。
    let two_opts = build_opts(
        &dir,
        vec![
            SourcePlan {
                agent: AgentKind::ClaudeCode,
                root: root.clone(),
                claude_mapping: None,
            },
            SourcePlan {
                agent: AgentKind::ClaudeCode,
                root: alias.clone(),
                claude_mapping: None,
            },
        ],
    );
    let t = Instant::now();
    let two_snap = query::begin_query(&two_opts).expect("双根采集失败");
    let two_ms = t.elapsed().as_millis();
    let two = query::query_summary(&two_snap.query_id).expect("双根汇总失败");

    // 结果与单根一致：请求数、四桶、费用全部相同，且等于合成规模。
    assert_totals_equal("双根/单根", &two.totals, &base.totals);
    assert_eq!(two.totals.requests, TOTAL, "重叠追加根不得重复计数");
    let (ein, eout, ecw, ecr) = claude_expected(SLUGS, FILES, EVENTS);
    assert_eq!(two.totals.tokens.input, ein);
    assert_eq!(two.totals.tokens.output, eout);
    assert_eq!(two.totals.tokens.cache_write, ecw);
    assert_eq!(two.totals.tokens.cache_read, ecr);

    let fp_after = real_tokenscope_fingerprint();
    assert_eq!(
        fp_before, fp_after,
        "真实 ~/.tokenscope 指纹变化：本测试不得写用户数据目录"
    );

    println!(
        "多根重叠（默认根 + 别名追加根）：{} 请求（与单根一致）；双根冷轮 begin {two_ms} ms（单根基线规模相同，耗时如实记录）",
        two.totals.requests
    );
    for s in &two.sources {
        println!(
            "  来源 {}: files_scanned {} / events_in_history {}",
            s.agent.as_str(),
            s.stats.files_scanned,
            s.stats.events
        );
    }
    println!(
        "Rust 堆峰值 {} MiB；证据目录：{}",
        peak_bytes() / (1024 * 1024),
        dir.display()
    );
}

// ── 指纹检查器的观察范围与盲区（合成目录，有区分度的自证） ─────────────

/// 指纹 = (文件数, 总字节数)。本用例在合成目录上钉住它能检测什么、
/// 检测不到什么：新增/删除/字节量变化会被发现；**同长度的内容改写
/// 不可检测**——这正是主流程指纹断言的观察范围边界，「逐字节零写入」
/// 不在其证明范围内（如实标注为未证明，见模块注释）。
#[test]
#[ignore = "AC09 N13 性能测试架：需显式设置 TOKENSCOPE_REAL_PERF=1（当前为合成来源阶段）"]
fn ac09_fingerprint_scope_and_blind_spot() {
    let _g = serial();
    if !gate_enabled() {
        eprintln!("跳过：未设置 TOKENSCOPE_REAL_PERF=1");
        return;
    }
    let dir = hermetic_dir("fp-scope");
    let sub = dir.join("nested");
    std::fs::create_dir_all(&sub).unwrap();

    // 空目录：存在但零文件。
    assert_eq!(fingerprint_dir(&dir), Some((0, 0)));
    // 新增文件：文件数与字节数变化可检测（含嵌套目录）。
    std::fs::write(sub.join("a.txt"), b"hello").unwrap(); // 5 B
    assert_eq!(fingerprint_dir(&dir), Some((1, 5)));
    // 同长度内容改写：不可检测（盲区，按文档钉住）。
    std::fs::write(sub.join("a.txt"), b"hellO").unwrap();
    assert_eq!(
        fingerprint_dir(&dir),
        Some((1, 5)),
        "同长度改写不改文件数/字节数——这正是指纹断言的观察范围边界"
    );
    // 字节量变化可检测（同文件数）。
    std::fs::write(sub.join("a.txt"), b"hello!").unwrap(); // 6 B
    assert_eq!(fingerprint_dir(&dir), Some((1, 6)));
    // 删除可检测。
    std::fs::remove_file(sub.join("a.txt")).unwrap();
    assert_eq!(fingerprint_dir(&dir), Some((0, 0)));
    let _ = std::fs::remove_dir_all(&dir);
}
