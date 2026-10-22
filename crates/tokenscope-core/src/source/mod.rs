//! source 层契约：各 agent 适配器把本地日志解析为归一化事件。
//! 适配器必须只读（M1 不变量 1）。M4 起：
//! - 事件为**未去重**形态，跨文件去重统一由 `crate::dedupe` 在 report 管线执行；
//! - 发现与解析拆为 `discover` / `parse_file` 两个能力，供缓存按文件指纹增量失效。

pub mod claude;
pub mod claude_projects;
pub mod codex;
pub mod discovery;
pub mod project_path;

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::model::{AgentKind, UsageEvent};

/// 一次采集的观测计数；坏行 / 去重 / 跳过口径见 M1/M2/M4 plan 不变量。
/// Codex 专属计数器为 0 时不序列化；`duplicates_dropped` 自 M4 起由全局去重
/// 步骤统计并回填，source 层恒为 0。`io_errors` 是文件级读取失败数（B4）：
/// 读取失败的文件不写成功缓存，且在来源统计中可见。
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct CollectStats {
    pub files_scanned: u64,
    pub lines_seen: u64,
    pub bad_lines: u64,
    pub duplicates_dropped: u64,
    pub skipped_sidechain: u64,
    pub skipped_synthetic: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_zero_usage: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_no_model: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub ignored_token_usage_record: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub io_errors: u64,
    pub events: u64,
}

impl CollectStats {
    /// PR3：累加另一份同粒度统计——来源按根实例采集后，报告按 agent 聚合用。
    pub fn merge_from(&mut self, other: &CollectStats) {
        self.files_scanned += other.files_scanned;
        self.lines_seen += other.lines_seen;
        self.bad_lines += other.bad_lines;
        self.duplicates_dropped += other.duplicates_dropped;
        self.skipped_sidechain += other.skipped_sidechain;
        self.skipped_synthetic += other.skipped_synthetic;
        self.skipped_zero_usage += other.skipped_zero_usage;
        self.skipped_no_model += other.skipped_no_model;
        self.ignored_token_usage_record += other.ignored_token_usage_record;
        self.io_errors += other.io_errors;
        self.events += other.events;
    }

    /// 累加单个文件的解析统计（files_scanned 与 events 由调用方维护）。
    pub fn add_file(&mut self, f: &FileParse) {
        self.lines_seen += f.stats.lines_seen;
        self.bad_lines += f.stats.bad_lines;
        self.skipped_sidechain += f.stats.skipped_sidechain;
        self.skipped_synthetic += f.stats.skipped_synthetic;
        self.skipped_zero_usage += f.stats.skipped_zero_usage;
        self.skipped_no_model += f.stats.skipped_no_model;
        self.ignored_token_usage_record += f.stats.ignored_token_usage_record;
        self.io_errors += f.stats.io_errors;
    }
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// jsonl 扩展名判断，供各适配器的发现逻辑共用。
pub fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
}

pub struct Collection {
    pub agent: AgentKind,
    pub events: Vec<UsageEvent>,
    pub stats: CollectStats,
    pub warnings: Vec<String>,
}

/// 单文件解析产物（缓存存取的基本单位）。
#[derive(Debug, Default, Clone)]
pub struct FileParse {
    pub stats: CollectStats,
    pub events: Vec<UsageEvent>,
}

pub trait Source {
    fn agent(&self) -> AgentKind;

    /// 扫描根目录（缺失警告与缓存展示用）。
    fn root(&self) -> &Path;

    /// H02：**全部**采集根（默认单根 = [`root`](Self::root)）。
    ///
    /// Codex 默认来源同时覆盖 `sessions/` 与 `archived_sessions/`；发现、
    /// 目录缺失判定与缓存上下文都按同一份根集合处理，避免"某一根被静默
    /// 忽略"或"换根后仍复用旧归属"。显式自定义根恒为单根——只在选定目录
    /// 内递归，不扩大到目录之外。
    fn roots(&self) -> Vec<PathBuf> {
        vec![self.root().to_path_buf()]
    }

    /// 发现全部 jsonl，按路径排序保证稳定顺序（B4）：子目录不可读等
    /// 发现期异常以诊断字符串返回，不再静默吞掉——"只统计到部分数据"
    /// 必须对调用方可见。
    ///
    /// 默认实现独立遍历（自带台账）；整轮采集由 report 层改调
    /// [`discover_with_ledger`](Self::discover_with_ledger) 共享台账。
    fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
        let mut ledger = DirVisitLedger::default();
        self.discover_with_ledger(&mut ledger)
    }

    /// PR3 §8.2：使用**整轮共享**的目录台账发现——环、别名根与同应用嵌套
    /// 根在整轮范围内只进入一次，文件由更小规范化根认领。适配器实现它；
    /// 注入来源（mock/测试）沿用 [`discover_with_errors`](Self::discover_with_errors)。
    fn discover_with_ledger(&self, _ledger: &mut DirVisitLedger) -> (Vec<PathBuf>, Vec<String>) {
        self.discover_with_errors()
    }

    /// 兼容便捷入口：只要文件列表。
    fn discover(&self) -> Vec<PathBuf> {
        self.discover_with_errors().0
    }

    /// 解析单个文件（不做跨文件去重；读取失败计 `io_errors` 并返回空产物）。
    fn parse_file(&self, path: &Path) -> FileParse;

    /// A03：解析上下文修订——影响解析结果的**外部配置**摘要（如 Claude 项目
    /// 映射）。变化即代表派生结果失效：文件缓存按该修订区分命中，查询复用键
    /// 也据此分开。默认空串 = 无可变外部依赖。
    fn context_revision(&self) -> &str {
        ""
    }

    /// 便捷全量采集：发现 + 逐文件解析，未全局去重（目录缺失 → 警告 + 空结果）。
    fn collect(&self) -> Result<Collection> {
        let mut stats = CollectStats::default();
        let mut warnings = Vec::new();
        let mut events = Vec::new();
        let roots = self.roots();
        if !roots.iter().any(|r| r.is_dir()) {
            for root in &roots {
                warnings.push(format!(
                    "{} 目录不存在：{}",
                    match self.agent() {
                        AgentKind::ClaudeCode => "Claude",
                        AgentKind::Codex => "Codex",
                    },
                    root.display()
                ));
            }
            return Ok(Collection {
                agent: self.agent(),
                events,
                stats,
                warnings,
            });
        }
        let (files, discovery_errors) = self.discover_with_errors();
        warnings.extend(discovery_errors);
        for path in files {
            stats.files_scanned += 1;
            let parsed = self.parse_file(&path);
            stats.add_file(&parsed);
            events.extend(parsed.events);
        }
        stats.events = events.len() as u64;
        Ok(Collection {
            agent: self.agent(),
            events,
            stats,
            warnings,
        })
    }
}

// ── PR3 §4.2/§8.2：防环遍历与整轮目录台账 ─────────────────────────

/// 整轮共享的目录访问台账：键 = 规范化目录路径（canonical 优先，不可访问退
/// 逻辑折叠），值 = 首个进入它的来源。环、别名根、同应用嵌套根靠它保证
/// "每个可解析目录整轮只进入一次"；跨应用根重叠在进入时暴露给调用方隔离。
#[derive(Debug, Default)]
pub struct DirVisitLedger {
    visited: std::collections::HashMap<String, AgentKind>,
}

/// 台账进入判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirEnter {
    /// 首次进入（调用方继续遍历）。
    Fresh,
    /// 已被访问过（同 agent 或跨 agent）——不再进入。
    Already(AgentKind),
}

impl DirVisitLedger {
    /// 登记进入一个目录：首次返回 `Fresh`，重复返回 `Already(首个来源)`。
    pub fn enter(&mut self, dir: &Path, agent: AgentKind) -> DirEnter {
        let key = crate::report::normalize_path(dir)
            .to_string_lossy()
            .to_lowercase();
        match self.visited.get(&key) {
            Some(prev) => DirEnter::Already(*prev),
            None => {
                self.visited.insert(key, agent);
                DirEnter::Fresh
            }
        }
    }
}

/// Windows reparse point 检测（§9.4 第 5 条）：junction 以 reparse point 形态
/// 存在，不能只假设 `file_type().is_symlink()` 覆盖全部联接。
#[cfg(windows)]
fn entry_is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    match std::fs::symlink_metadata(path) {
        Ok(md) => md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn entry_is_reparse_point(_path: &Path) -> bool {
    false
}

/// 显式栈遍历（§4.2）：`DirEntry::file_type()` 不跟随分型；链接（符号链接与
/// Windows 联接）显式解析目标——目标是目录时按规范化目标进台账（防环、防
/// 别名重复），目标是 jsonl 文件时按访问路径收集（身份去重在文件层完成）；
/// 无法解析的链接跳过并诊断。普通目录进入前也进台账。子项读取/类型错误
/// 逐项保留，不用 `entries.flatten()` 静默吞错。
pub fn walk_jsonl_guarded(
    root: &Path,
    agent: AgentKind,
    ledger: &mut DirVisitLedger,
    out: &mut Vec<PathBuf>,
    errors: &mut Vec<String>,
) {
    #[cfg(feature = "acceptance")]
    if let Err(error) = crate::acceptance::record_path(
        "source_root_read",
        root,
        serde_json::json!({"agent": agent.as_str()}),
    ) {
        errors.push(format!("验收来源隔离检查失败：{error:#}"));
        return;
    }
    // 根进入台账：跨应用根重叠在此暴露（调用方按隔离处理），同应用别名根
    // 的文件由更小规范化根认领。
    match ledger.enter(root, agent) {
        DirEnter::Already(prev) => {
            if prev != agent {
                errors.push(format!(
                    "根 {} 已被 {} 来源遍历：跨应用来源根重叠，本轮隔离该根",
                    root.display(),
                    prev.as_str()
                ));
            }
            return;
        }
        DirEnter::Fresh => {}
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!("无法读取目录 {}：{e}", dir.display()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(format!("读取目录 {} 子项失败：{e}", dir.display()));
                    continue;
                }
            };
            let child = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(e) => {
                    errors.push(format!("读取 {} 类型失败：{e}", child.display()));
                    continue;
                }
            };
            // 链接：符号链接，或 Windows 上带 reparse 属性的目录（junction）
            let is_link =
                file_type.is_symlink() || (file_type.is_dir() && entry_is_reparse_point(&child));
            if is_link {
                // 链接显式解析目标（§4.2）：目录目标按规范化路径进台账后压栈
                // （台账挡下环与别名重复）；文件目标按访问路径收集，身份去重
                // 在文件层完成；解析失败跳过并诊断，绝不能无界递归。
                let target = match child.canonicalize() {
                    Ok(t) => crate::report::strip_verbatim(t),
                    Err(_) => {
                        errors.push(format!(
                            "链接 {} 目标无法解析（失效链接），已跳过",
                            child.display()
                        ));
                        continue;
                    }
                };
                let meta = match std::fs::metadata(&target) {
                    Ok(m) => m,
                    Err(e) => {
                        errors.push(format!("链接 {} 目标不可读：{e}", child.display()));
                        continue;
                    }
                };
                if meta.is_dir() {
                    match ledger.enter(&target, agent) {
                        DirEnter::Fresh => stack.push(target),
                        DirEnter::Already(_) => {}
                    }
                } else if is_jsonl(&child) {
                    out.push(child);
                }
                continue;
            }
            if file_type.is_dir() {
                // 普通目录也在进入前加入台账（§9.4 第 3 条）——祖先环由此终止
                match ledger.enter(&child, agent) {
                    DirEnter::Fresh => stack.push(child),
                    DirEnter::Already(_) => {}
                }
            } else if file_type.is_file() && is_jsonl(&child) {
                out.push(child);
            }
        }
    }
}

pub(crate) fn read_text(path: &Path) -> Result<String, std::io::Error> {
    // lossy 容错：个别非法字节不应让整份文件失败，垃圾行由坏行计数接住。
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}
