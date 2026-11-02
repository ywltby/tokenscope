//! AC09 N13 真实规模性能（收口计划 §4「AC09 N13 真实规模性能（本地）」）。
//!
//! 默认 `#[ignore]`，不进常规套件与提交钩子；显式运行需 `TOKENSCOPE_REAL_PERF=1`：
//!
//! ```text
//! TOKENSCOPE_REAL_PERF=1 cargo test --release --test perf_history_real -- --ignored --nocapture
//! ```
//!
//! **阶段 1（合成来源）**：验证测试架本身——冷/热采集、首页汇总与明细前两页
//! 可测量；冷热请求数与四桶一致；默认根与重叠追加根不重复计数；隔离不变量成立。
//!
//! **阶段 2（真实来源，只读）**：`ac09_real_sources_cold_warm_pipeline` 只读
//! `~/.claude/projects` 与 `~/.codex/sessions`，写入全部落在本次 hermetic
//! 临时目录。这里的"只读"指**不写入、不修改、不删除来源文件**，不等于
//! "不读取内容"：采集必然把日志解析进内存（这是产品行为），临时历史库也会
//! 保存来源路径（cwd/项目目录等）与身份元数据——两者都只存在于本次临时目录，
//! 随临时根清理删除，不入库、不输出。真正只读**元数据**的只有目录指纹步骤
//! （文件数、字节数、最新 mtime）。
//!
//! 旧 `perf_real_data.rs` 不改不删（它是 CLAUDE.md 记载的真实缓存预热工具，
//! 会写 `~/.tokenscope`），N13 的结论只取自本文件，不引用旧测试的数字。
//!
//! 隔离不变量（AC09 要求，全部显式注入）：
//! - 历史库 `history_path`、缓存 `cache_dir`、价格索引 `pricing_index`、
//!   价格文件 `pricing_path` 与证据输出全部落在 hermetic 临时目录；
//! - OpenRouter/models.dev 快照注入不存在的路径（不读、不写真实快照）；
//! - 来源计划 `source_plans` 显式注入：`Some` 完全取代 `claude_dir`/
//!   `codex_dir` 的默认根语义，默认根（`~/.claude` 等）不会被咨询；阶段 1
//!   注入合成根并断言全部位于隔离目录内；阶段 2 显式注入真实来源根（见下），
//!   其余 dir 类字段仍指向隔离目录内的不存在路径，即使未来回归到 dir 语义
//!   也不会意外落在真实用户目录；`claude_mapping: None` 不读 `~/.claude.json`。
//!
//! 证据边界（复核收窄，如实标注——观察范围之外的部分为**未证明**）：
//! - 写侧：运行前后对真实 `~/.tokenscope` 取指纹且必须不变（本机无该
//!   目录则必须始终不存在，本测试绝不新建它）。指纹只比较**文件数与
//!   总字节数**——同长度的内容改写不可检测（敏感性用例
//!   `ac09_fingerprint_scope_and_blind_spot` 已在合成目录钉住该边界）；
//!   「逐字节零写入」未证明。
//! - 读侧（阶段 1）：请求数精确等于合成规模、四桶与期望公式吻合、按项目
//!   分组的**身份集合**与 fixture 指定的 26 个合成项目逐项一致——只能排除
//!   **在全局去重后仍新增事件的额外来源**；与已有事件同原生身份而被去重的
//!   额外来源、空目录或零有效事件来源的读取不可由结果排除，结构性保证是
//!   `source_plans` 显式注入（见上）。
//! - 读侧（阶段 2，如实标注）：采集**会读取来源日志内容到内存**（解析所必需），
//!   临时历史库**会保存来源路径与项目目录等元数据**；"来源未被写入/修改/删除"
//!   由不写盘的代码路径与运行前后来源指纹保证，但"零读取"或"从未保存路径"
//!   **不成立、也不在本测试的证明范围内**。本测试只对真实 `~/.tokenscope`
//!   读取目录元数据（文件数 + 总字节数），不读其文件内容，不输出文件名清单。
//! - 分页：前两页与合成数据的**独立期望顺序逐位核对**（不重复也不
//!   漏行），并以「跳过中间一段」的反例证明核对对漏行有区分力。
//!
//! 测量（只记录，不设硬门槛）：各阶段耗时（细分阶段由 collect_all 的 INFO
//! 日志给出，--nocapture 可见）、事件与文件数量（来源统计）、Rust 堆峰值
//! （N05 计数分配器；不覆盖 SQLite C 分配与完整 RSS，见
//! docs/stats-semantics.md §3.5）、进程峰值工作集（Windows 为进程记录的
//! 峰值经外部 PowerShell 查询，无采样间隙；不可获得时如实标注）。
//! 证据 JSON 写入隔离目录（不入库），只含汇总数字与边界说明；不输出日志正文、
//! 文件名或项目路径。证据随临时根清理删除（本轮实测未另存原始证据）。

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

/// 合成项目身份集合（项目 key = 规范化 cwd）：Claude 6 个
/// `C:/perf/work{slug}` + Codex 20 个 `C:/perf/x{file}`。只比较分组数量
/// 不能证明「恰为指定项目」，核对时必须按集合逐项比较。
fn expected_project_keys() -> Vec<String> {
    let mut keys: Vec<String> = (0..CLAUDE_SLUGS)
        .map(|slug| format!("C:/perf/work{slug}"))
        .collect();
    keys.extend((0..CODEX_FILES).map(|file| format!("C:/perf/x{file}")));
    keys
}

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

/// 把证据 JSON 写到 hermetic 目录；若设置了 `TOKENSCOPE_PERF_EVIDENCE_DIR`
/// （gitignored 的 qa-artifacts 子目录），再复制一份到那里，供跨临时根复查。
/// 只保存汇总数字与边界说明——调用方必须保证 JSON 不含会话正文、文件名、
/// 项目路径或任何路径字符串（合成用例 `ac09_perf_evidence_redaction` 钉住）。
fn write_perf_evidence(dir: &Path, name: &str, evidence: &serde_json::Value) -> Vec<PathBuf> {
    let body = serde_json::to_string_pretty(evidence).unwrap();
    let mut written = Vec::new();
    let local = dir.join(name);
    std::fs::write(&local, &body).unwrap();
    written.push(local);
    if let Some(dest) = std::env::var_os("TOKENSCOPE_PERF_EVIDENCE_DIR") {
        let dest = PathBuf::from(dest);
        std::fs::create_dir_all(&dest).unwrap();
        let out = dest.join(name);
        std::fs::write(&out, &body).unwrap();
        written.push(out);
    }
    written
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

    // ── 读侧有区分度检查：项目分组的身份集合逐项等于 fixture 指定的
    // 26 个合成项目（6 Claude + 20 Codex），请求数合计等于合成规模——
    // 这只能排除**在全局去重后仍新增事件的额外来源**；与已有事件同
    // 原生身份而被去重的额外来源、空来源/零有效事件来源不可由此排除。
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
    let mut actual_keys: Vec<String> = real_groups.iter().map(|g| g.key.clone()).collect();
    actual_keys.sort();
    let mut expected_keys = expected_project_keys();
    expected_keys.sort();
    assert_eq!(
        actual_keys, expected_keys,
        "项目身份集合应与 fixture 指定的 26 个合成项目逐项一致（仅比较数量不足）"
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
    // 改写不可检测）；请求数与项目身份集合只排除「在全局去重后仍新增
    // 事件的额外来源」——与已有事件同原生身份而被去重的额外来源、空来源
    // 无法由结果排除；结构性保证是 source_plans 显式注入合成根（见上方
    // 来源根断言）。真实来源阶段的读取范围与写入口径见阶段 2 用例注释
    // （采集会读取日志内容、临时历史库保存来源路径元数据，均为如实标注）。

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
            "all_writes_under_hermetic_temp_dir": true,
            "real_tokenscope_present_before": fp_before.is_some(),
            "real_tokenscope_fingerprint_unchanged": true,
            "fingerprint_scope": "files_and_bytes_only（同长度内容改写不可检测）",
        },
        "verification": {
            "page_rows_checked": 2 * PAGE,
            "expected_order_match": true,
            "skip_counterexample_rejected": true,
            "project_groups": real_groups.len(),
            "project_identity_match": true,
            "project_requests_sum": proj_requests,
        },
    });
    let written = write_perf_evidence(&dir, "ac09-stage1-evidence.json", &evidence);

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
    println!(
        "证据 JSON：{}（另存目录 TOKENSCOPE_PERF_EVIDENCE_DIR={}）",
        written[0].display(),
        std::env::var_os("TOKENSCOPE_PERF_EVIDENCE_DIR")
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<未设置，不另存>".into())
    );
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

// ── 阶段 2：真实来源（只读 ~/.claude、~/.codex；全部写入隔离目录） ─────
//
// 与阶段 1（合成来源）严格分开：本阶段的数字只来自真实来源树，**不引用
// 合成结果**；缺失真实来源时跳过并如实标注，不伪造。
//
// 只读与写入边界（如实口径）：
// - **只读 = 不写入、不修改、不删除来源文件**；**不是"不读取内容"**：采集会把
//   来源日志解析进内存（产品行为），临时历史库会保存来源路径与项目目录等元数据
//   ——两者都只在本次 hermetic 临时目录，随临时根清理删除，不入库、不输出。
// - 真正只读**元数据**的只有目录指纹（文件数、字节数、最新 mtime）；不输出
//   文件名、项目路径或正文；`claude_mapping = None` 不读 `~/.claude.json`。
// - 历史库、缓存、价格索引、价格表与证据 JSON 全部落在本用例的 hermetic
//   临时目录；真实 `~/.tokenscope` 只取运行前后指纹（文件数 + 总字节数），
//   必须不变，且本用例绝不新建该目录。
// - 来源树在测量期间若发生变化（文件数/字节数/最新 mtime 任一有变），
//   先记录观察结果再判定一致性，并如实标注 `source_tree_changed_*`，
//   不为此反复重跑。

/// 合成验证：证据的保存、另存与脱敏流程（不读真实来源）。
/// 钉住"证据只含汇总数字"的要求：序列化文本不得含 hermetic 目录路径、
/// Windows 路径分隔或来源字段；另存与清理都必须发生。
#[test]
#[ignore = "AC09 N13 合成定向验证：需 TOKENSCOPE_REAL_PERF=1（不读真实来源）"]
fn ac09_perf_evidence_redaction() {
    let _g = serial();
    if !gate_enabled() {
        eprintln!("跳过：未设置 TOKENSCOPE_REAL_PERF=1");
        return;
    }
    let dir = hermetic_dir("evidence-redaction");
    let out_dir = hermetic_dir("evidence-out");
    // 保存流程：设置另存目录 → 写证据 → 两处都有。
    // set_var/remove_var 在 edition 2024 是 unsafe：本用例持 serial() 锁、
    // 单线程顺序执行，进程内没有并发读者，因此安全。
    unsafe {
        std::env::set_var("TOKENSCOPE_PERF_EVIDENCE_DIR", &out_dir);
    }
    let evidence = serde_json::json!({
        "phase": "合成脱敏验证",
        "totals": { "requests": 1, "input": 1, "output": 1, "cache_write": 1, "cache_read": 1 },
        "isolation": { "all_writes_under_hermetic_temp_dir": true },
        "boundaries": ["不含会话正文、文件名、项目路径或来源路径"],
    });
    let written = write_perf_evidence(&dir, "ac09-evidence-redaction.json", &evidence);
    assert_eq!(written.len(), 2, "hermetic 目录与另存目录各写一份");

    // 脱敏检查：文本不得包含 hermetic 目录路径或 Windows 路径分隔，
    // 不得包含"history_path"等路径字段。
    let body = std::fs::read_to_string(&written[1]).unwrap();
    assert!(
        !body.contains(dir.to_string_lossy().as_ref()),
        "证据不得包含 hermetic 目录路径"
    );
    assert!(!body.contains('\\'), "证据不得包含 Windows 路径分隔");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        parsed
            .get("isolation")
            .and_then(|iso| iso.get("history_path"))
            .is_none(),
        "证据不得包含 history_path 字段"
    );
    assert_eq!(
        parsed["isolation"]["all_writes_under_hermetic_temp_dir"],
        serde_json::json!(true)
    );

    // 清理流程：删掉两份证据与两个目录，目录必须消失（复制 run_rust_tests 的
    // 收尾语义：删除后核对）。
    for p in &written {
        std::fs::remove_file(p).unwrap();
    }
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out_dir).unwrap();
    assert!(!dir.exists(), "hermetic 目录应已清理");
    assert!(!out_dir.exists(), "另存目录应已清理");
    unsafe {
        std::env::remove_var("TOKENSCOPE_PERF_EVIDENCE_DIR");
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// 真实来源根：Claude `~/.claude/projects`、Codex `~/.codex/sessions`。
/// 只判断存在性，不读内容、不输出路径。
fn real_source_roots() -> Option<(PathBuf, PathBuf)> {
    let home = home_dir()?;
    let claude = home.join(".claude").join("projects");
    let codex = home.join(".codex").join("sessions");
    if !claude.is_dir() || !codex.is_dir() {
        return None;
    }
    Some((claude, codex))
}

/// 来源树指纹：(文件数, 总字节数, 最新 mtime 纳秒)。只读目录元数据，
/// 不读文件内容、不输出文件名——用于检测测量期间来源是否变化。
fn source_tree_fingerprint(dir: &Path) -> Option<(u64, u64, u128)> {
    if !dir.exists() {
        return None;
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut newest: u128 = 0;
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
                if let Ok(meta) = entry.metadata() {
                    bytes += meta.len();
                    if let Ok(Ok(dur)) = meta
                        .modified()
                        .map(|m| m.duration_since(std::time::UNIX_EPOCH))
                    {
                        newest = newest.max(dur.as_nanos());
                    }
                }
            }
        }
    }
    Some((files, bytes, newest))
}

/// 一轮真实来源测量。只记录数字，不设任何性能门槛。
#[derive(Debug, Default, Clone, serde::Serialize)]
struct RealRound {
    begin_ms: u128,
    summary_ms: u128,
    page1_ms: u128,
    page2_ms: u128,
    requests: u64,
    input: u64,
    output: u64,
    cache_write: u64,
    cache_read: u64,
    cost_usd: f64,
    page1_rows: usize,
    page2_rows: usize,
    page1_total: u64,
    files_scanned: u64,
    events_in_history: u64,
    rust_heap_peak_bytes: usize,
    process_peak_working_set_bytes: Option<u64>,
}

/// 来源变化对测量结果的影响说明（空串 = 未观察到变化）。
/// 断言消息必须包含它：来源在测量期间变化时，结果标为受影响，
/// 不得静默按"未变化"口径记通过。
fn source_change_note(changed: bool) -> &'static str {
    if changed {
        "；来源在测量期间发生变化，本结果受影响（不得按未变化口径解读）"
    } else {
        ""
    }
}

fn measure_real_round(label: &str, opts: &SummaryOptions) -> RealRound {
    reset_peak();
    let mut r = RealRound::default();

    let t = Instant::now();
    let snap =
        query::begin_query_with(opts, true).unwrap_or_else(|e| panic!("{label} 采集失败: {e}"));
    r.begin_ms = t.elapsed().as_millis();
    r.rust_heap_peak_bytes = peak_bytes();

    let t = Instant::now();
    let sum =
        query::query_summary(&snap.query_id).unwrap_or_else(|e| panic!("{label} 汇总失败: {e}"));
    r.summary_ms = t.elapsed().as_millis();
    r.requests = sum.totals.requests;
    r.input = sum.totals.tokens.input;
    r.output = sum.totals.tokens.output;
    r.cache_write = sum.totals.tokens.cache_write;
    r.cache_read = sum.totals.tokens.cache_read;
    r.cost_usd = sum.totals.cost_usd;
    for s in &sum.sources {
        r.files_scanned += s.stats.files_scanned;
        r.events_in_history += s.stats.events;
    }

    let t = Instant::now();
    let page1 = query::query_events(&snap.query_id, &EventFilter::default())
        .unwrap_or_else(|e| panic!("{label} 明细第一页失败: {e}"));
    r.page1_ms = t.elapsed().as_millis();
    r.page1_rows = page1.rows.len();
    r.page1_total = page1.total;
    assert_eq!(page1.total, r.requests, "{label}：明细总数应等于汇总请求数");

    let t = Instant::now();
    if let Some(last) = page1.rows.last() {
        let page2 = query::query_events(
            &snap.query_id,
            &EventFilter {
                before: Some(last.cursor.clone()),
                ..EventFilter::default()
            },
        )
        .unwrap_or_else(|e| panic!("{label} 明细第二页失败: {e}"));
        r.page2_ms = t.elapsed().as_millis();
        r.page2_rows = page2.rows.len();
        let seen: std::collections::HashSet<&str> =
            page1.rows.iter().map(|x| x.cursor.as_str()).collect();
        assert!(
            page2.rows.iter().all(|x| !seen.contains(x.cursor.as_str())),
            "{label}：翻页不得重复"
        );
    }
    r.process_peak_working_set_bytes = process_peak_working_set_bytes();
    r
}

/// 合成定向验证：来源指纹变化检测与"测量受影响"标注（不读真实来源）。
/// 只用本次 hermetic 目录里的合成文件，验证顺序要求的最小前提：
/// 指纹能检出变化，且发生变化时标注必须写明"结果受影响"。
#[test]
#[ignore = "AC09 N13 合成定向验证：需 TOKENSCOPE_REAL_PERF=1（不读真实来源）"]
fn ac09_source_change_note_marks_measurement_affected() {
    let _g = serial();
    if !gate_enabled() {
        eprintln!("跳过：未设置 TOKENSCOPE_REAL_PERF=1");
        return;
    }
    let dir = hermetic_dir("source-change");
    let tree = dir.join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::write(tree.join("a.jsonl"), b"{}").unwrap();

    let before = source_tree_fingerprint(&tree);
    assert!(before.is_some(), "合成来源树必须可被指纹读取");
    assert_eq!(source_change_note(false), "", "未变化时不得拼接影响说明");
    // 无变化：指纹稳定，note 仍为空（正控制）。
    assert_eq!(source_tree_fingerprint(&tree), before);
    assert_eq!(source_change_note(false), "");

    // 追加字节 → 指纹变化 → note 必须明确标注受影响。
    std::fs::write(tree.join("a.jsonl"), b"{} \n").unwrap();
    let after = source_tree_fingerprint(&tree);
    assert_ne!(after, before, "追加字节必须被指纹检测到");
    let note = source_change_note(after != before);
    assert!(
        note.contains("来源在测量期间发生变化"),
        "note 必须说明来源变化"
    );
    assert!(note.contains("结果受影响"), "note 必须标注结果受影响");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 真实来源：冷/热采集、首页汇总、明细前两页、四桶一致性、重叠根不重复计数。
/// 只记录数字，不设门槛；来源缺失即跳过，不伪造结果。
#[test]
#[ignore = "AC09 N13 真实来源性能：需显式设置 TOKENSCOPE_REAL_PERF=1"]
fn ac09_real_sources_cold_warm_pipeline() {
    let _g = serial();
    if !gate_enabled() {
        eprintln!("跳过：未设置 TOKENSCOPE_REAL_PERF=1");
        return;
    }
    // 验收覆盖会让默认数据目录指向验收根，真实 ~/.tokenscope 指纹不再可信。
    if std::env::var_os("TOKENSCOPE_ACCEPTANCE_ROOT").is_some() {
        eprintln!("跳过：检测到 TOKENSCOPE_ACCEPTANCE_ROOT，真实数据目录指纹不适用");
        return;
    }
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_ansi(false)
        .try_init();

    let Some((claude_root, codex_root)) = real_source_roots() else {
        eprintln!("跳过：真实来源根不存在（~/.claude/projects 或 ~/.codex/sessions）");
        return;
    };

    // 只读指纹：真实数据目录 + 两个来源树。
    let data_before = real_tokenscope_fingerprint();
    let claude_before = source_tree_fingerprint(&claude_root);
    let codex_before = source_tree_fingerprint(&codex_root);

    let dir = hermetic_dir("real");
    write_pricing(&dir);
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
    let opts = build_opts(&dir, plans.clone());

    // ── 冷轮（空历史库 → 全量解析入库）与热轮（来源指纹未变，不重解析）
    let cold = measure_real_round("真实冷轮", &opts);
    let warm = measure_real_round("真实热轮", &opts);

    // ── 来源观察先于一致性判定：冷/热跑完立刻取指纹，即使下面断言失败，
    //    panic 消息也带上"来源是否变化"，不把变化静默记成"不一致=缺陷"。
    let claude_after_cold_warm = source_tree_fingerprint(&claude_root);
    let codex_after_cold_warm = source_tree_fingerprint(&codex_root);
    let changed_cold_warm =
        claude_before != claude_after_cold_warm || codex_before != codex_after_cold_warm;
    if changed_cold_warm {
        eprintln!(
            "观察：冷/热测量期间来源树发生变化（文件数/字节数/最新 mtime 有变）——\
             后续一致性判定的失败可能由此产生，结果按受影响标注，不重跑"
        );
    }

    // ── 一致性：请求数与四桶必须完全一致（真实来源下不预设规模）
    assert_eq!(
        cold.requests,
        warm.requests,
        "真实冷/热请求数必须一致{}",
        source_change_note(changed_cold_warm)
    );
    assert_eq!(
        cold.input,
        warm.input,
        "真实冷/热 input 必须一致{}",
        source_change_note(changed_cold_warm)
    );
    assert_eq!(
        cold.output,
        warm.output,
        "真实冷/热 output 必须一致{}",
        source_change_note(changed_cold_warm)
    );
    assert_eq!(
        cold.cache_write,
        warm.cache_write,
        "真实冷/热 cache_write 必须一致{}",
        source_change_note(changed_cold_warm)
    );
    assert_eq!(
        cold.cache_read,
        warm.cache_read,
        "真实冷/热 cache_read 必须一致{}",
        source_change_note(changed_cold_warm)
    );
    // 分页：满页只在总数足够时要求（真实规模不预设）。
    if cold.requests > (2 * PAGE) as u64 {
        assert_eq!(
            cold.page1_rows,
            PAGE,
            "真实明细第一页应为满页{}",
            source_change_note(changed_cold_warm)
        );
        assert_eq!(
            cold.page2_rows,
            PAGE,
            "真实明细第二页应为满页{}",
            source_change_note(changed_cold_warm)
        );
    }

    // ── 重叠根：同一 Claude 根注入两次（同物理目录）必须与单根结果一致
    let single_dir = hermetic_dir("real-overlap-single");
    write_pricing(&single_dir);
    let single_opts = build_opts(&single_dir, plans.clone());
    let single = measure_real_round("真实单根", &single_opts);

    let double_dir = hermetic_dir("real-overlap-double");
    write_pricing(&double_dir);
    let mut double_plans = plans.clone();
    double_plans.push(SourcePlan {
        agent: AgentKind::ClaudeCode,
        root: claude_root.clone(),
        claude_mapping: None,
    });
    let double_opts = build_opts(&double_dir, double_plans);
    let double = measure_real_round("真实重叠双根", &double_opts);

    // 来源观察先于重叠一致性判定（累计到此刻的变化）。
    let claude_after_overlap = source_tree_fingerprint(&claude_root);
    let codex_after_overlap = source_tree_fingerprint(&codex_root);
    let changed_overlap =
        claude_before != claude_after_overlap || codex_before != codex_after_overlap;
    if changed_overlap && !changed_cold_warm {
        eprintln!("观察：重叠根测量期间来源树发生变化——重叠对比结果按受影响标注，不重跑");
    }

    assert_eq!(
        single.requests,
        double.requests,
        "重叠追加根不得重复计数：请求数必须与单根一致{}",
        source_change_note(changed_overlap)
    );
    assert_eq!(
        single.input,
        double.input,
        "重叠根 input 必须一致{}",
        source_change_note(changed_overlap)
    );
    assert_eq!(
        single.output,
        double.output,
        "重叠根 output 必须一致{}",
        source_change_note(changed_overlap)
    );
    assert_eq!(
        single.cache_write,
        double.cache_write,
        "重叠根 cache_write 必须一致{}",
        source_change_note(changed_overlap)
    );
    assert_eq!(
        single.cache_read,
        double.cache_read,
        "重叠根 cache_read 必须一致{}",
        source_change_note(changed_overlap)
    );

    // ── 项目分组数（claude_mapping 禁用 → Claude 按 slug 计；只记数量）
    let mut proj_opts = build_opts(&dir, plans.clone());
    proj_opts.by = GroupBy::Project;
    let proj_snap = query::begin_query_with(&proj_opts, false).expect("项目分组复用历史失败");
    let proj = query::query_summary(&proj_snap.query_id).expect("项目分组汇总失败");
    let groups = proj.groups.iter().filter(|g| g.key != "合计").count();

    // ── 来源变化总览（冷/热与重叠两段观察的并集；已在各段判定前记录）
    let source_changed = changed_cold_warm || changed_overlap;

    // ── 写侧隔离：真实 ~/.tokenscope 指纹必须不变
    let data_after = real_tokenscope_fingerprint();
    assert_eq!(
        data_before, data_after,
        "真实 ~/.tokenscope 指纹变化（{data_before:?} → {data_after:?}）：\
         本测试不得写入用户数据目录"
    );
    assert!(
        dir.join("history.db").is_file(),
        "历史库必须落在注入的 history_path"
    );
    assert!(
        dir.join("pricing-index.json").is_file(),
        "价格索引必须落在注入的 pricing_index"
    );

    // ── 证据 JSON（只含汇总数字与边界说明，不含文件名/路径/正文）
    let evidence = serde_json::json!({
        "phase": "AC09 阶段2（真实来源，只读；N13 开放）",
        "verification": {
            "cold_warm_requests_equal": true,
            "cold_warm_buckets_equal": true,
            "overlap_roots_counts_once": true,
            "page1_total_matches_requests": true,
            "page_rows": cold.page1_rows + cold.page2_rows,
            "project_groups": groups,
            "source_tree_changed_during_measurement": source_changed,
            "source_tree_change_checks": {
                "after_cold_warm": changed_cold_warm,
                "after_overlap": changed_overlap,
            },
            "real_tokenscope_fingerprint_unchanged": true,
        },
        "cold": cold,
        "warm": warm,
        "overlap_single": single,
        "overlap_double": double,
        "isolation": {
            "all_writes_under_hermetic_temp_dir": true,
            "source_files_written": false,
            "content_read_for_parsing": true,
            "temp_history_stores_source_metadata": true,
            "claude_mapping": "disabled（不读 ~/.claude.json）",
            "fingerprint_scope": "files_and_bytes_and_max_mtime",
        },
        "boundaries": [
            "来源只读 = 不写入/不修改/不删除来源文件；采集会读取内容到内存，临时历史库保存来源路径与项目目录等元数据（只在本次临时目录）",
            "只读元数据的只有目录指纹（文件数/字节数/最新 mtime）",
            "项目身份：claude_mapping=None，Claude 项目按 slug 计，组数不代表真实项目数",
            "成本：只加载注入的隔离价格表，真实模型可能未命中，cost 仅作参考",
            "堆峰值只统计 Rust 分配（计数分配器），不含 SQLite C 分配",
            "进程峰值工作集为进程级参考值，不区分阶段",
            "来源若在测量期间变化，冷/热与重叠对比可能受并发写入影响",
        ],
    });
    let written = write_perf_evidence(&dir, "ac09-stage2-real-evidence.json", &evidence);

    println!(
        "真实来源规模：files_scanned {} / events_in_history {} / 汇总请求 {}",
        cold.files_scanned, cold.events_in_history, cold.requests
    );
    println!(
        "冷轮：begin {} ms（采集）/ 汇总 {} ms / 两页 {} + {} ms（{} + {} 行）",
        cold.begin_ms,
        cold.summary_ms,
        cold.page1_ms,
        cold.page2_ms,
        cold.page1_rows,
        cold.page2_rows
    );
    println!(
        "热轮：begin {} ms / 汇总 {} ms / 两页 {} + {} ms（{} + {} 行）",
        warm.begin_ms,
        warm.summary_ms,
        warm.page1_ms,
        warm.page2_ms,
        warm.page1_rows,
        warm.page2_rows
    );
    println!(
        "四桶：input {} / output {} / cache_write {} / cache_read {}；费用 ${:.4}（隔离价格表）",
        cold.input, cold.output, cold.cache_write, cold.cache_read, cold.cost_usd
    );
    println!(
        "重叠根：单根 {} 请求 vs 双根 {} 请求（一致）；单根 begin {} ms / 双根 begin {} ms",
        single.requests, double.requests, single.begin_ms, double.begin_ms
    );
    println!(
        "Rust 堆峰值：冷 {} MiB / 热 {} MiB / 单根 {} MiB / 双根 {} MiB（不含 SQLite C 分配）",
        cold.rust_heap_peak_bytes / (1024 * 1024),
        warm.rust_heap_peak_bytes / (1024 * 1024),
        single.rust_heap_peak_bytes / (1024 * 1024),
        double.rust_heap_peak_bytes / (1024 * 1024)
    );
    println!(
        "进程峰值工作集：{}",
        match cold.process_peak_working_set_bytes {
            Some(v) => format!("{} MiB（参考值）", v / (1024 * 1024)),
            None => "不可获得（如实标注）".to_string(),
        }
    );
    println!(
        "项目分组数（claude_mapping 禁用，按 slug）：{groups}；来源在测量期间变化：{source_changed}"
    );
    println!(
        "证据 JSON：{}（另存目录 TOKENSCOPE_PERF_EVIDENCE_DIR={}）",
        written[0].display(),
        std::env::var_os("TOKENSCOPE_PERF_EVIDENCE_DIR")
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<未设置，不另存>".into())
    );
}
