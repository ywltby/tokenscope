//! source 层契约：各 agent 适配器把本地日志解析为归一化事件。
//! 适配器必须只读（M1 不变量 1）。M4 起：
//! - 事件为**未去重**形态，跨文件去重统一由 `crate::dedupe` 在 report 管线执行；
//! - 发现与解析拆为 `discover` / `parse_file` 两个能力，供缓存按文件指纹增量失效。

pub mod claude;
pub mod codex;

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::model::{AgentKind, UsageEvent};

/// 一次采集的观测计数；坏行 / 去重 / 跳过口径见 M1/M2/M4 plan 不变量。
/// Codex 专属计数器为 0 时不序列化；`duplicates_dropped` 自 M4 起由全局去重
/// 步骤统计并回填，source 层恒为 0。`io_errors` 是文件级读取失败数（B4）：
/// 读取失败的文件不写成功缓存，且在来源统计中可见。
#[derive(Debug, Default, Clone, Serialize)]
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

    /// 发现全部 jsonl，按路径排序保证稳定顺序（B4）：子目录不可读等
    /// 发现期异常以诊断字符串返回，不再静默吞掉——"只统计到部分数据"
    /// 必须对调用方可见。
    fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>);

    /// 兼容便捷入口：只要文件列表。
    fn discover(&self) -> Vec<PathBuf> {
        self.discover_with_errors().0
    }

    /// 解析单个文件（不做跨文件去重；读取失败计 `io_errors` 并返回空产物）。
    fn parse_file(&self, path: &Path) -> FileParse;

    /// 便捷全量采集：发现 + 逐文件解析，未全局去重（目录缺失 → 警告 + 空结果）。
    fn collect(&self) -> Result<Collection> {
        let mut stats = CollectStats::default();
        let mut warnings = Vec::new();
        let mut events = Vec::new();
        if !self.root().is_dir() {
            warnings.push(format!(
                "{} 目录不存在：{}",
                match self.agent() {
                    AgentKind::ClaudeCode => "Claude",
                    AgentKind::Codex => "Codex",
                },
                self.root().display()
            ));
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

/// 递归收集 root 下全部 jsonl，按路径排序（Claude/Codex 共用）。
/// 不可读的目录（含"同名文件占位"等异常）记入 errors，不中断其余遍历。
pub(crate) fn walk_jsonl(root: &Path, out: &mut Vec<PathBuf>, errors: &mut Vec<String>) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => {
            errors.push(format!("无法读取目录 {}：{e}", root.display()));
            return;
        }
    };
    let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            walk_jsonl(&child, out, errors);
        } else if is_jsonl(&child) {
            out.push(child);
        }
    }
}

pub(crate) fn read_text(path: &Path) -> Result<String, std::io::Error> {
    // lossy 容错：个别非法字节不应让整份文件失败，垃圾行由坏行计数接住。
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}
