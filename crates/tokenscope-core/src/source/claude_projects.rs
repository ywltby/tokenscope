//! A03：Claude 项目映射——把历史 slug 身份**正向**解析回真实路径。
//!
//! 依据：`~/.claude.json` 的 `projects` 键保存该账户见过的项目绝对路径，而
//! Claude Code 把路径编码为 `projects/<slug>/` 目录名，编码规则 = 每个非
//! ASCII 字母数字字符替换为一个 `-`（本机 8/8 slug 与账户配置逐条吻合）。
//!
//! 口径：
//! - 只用**正向编码**生成候选，绝不反向解码 slug，也不按路径前缀猜测；
//! - 同一 slug 对应多个路径（归一化后仍不同）→ 冲突：该 slug 保持独立身份，
//!   不选择第一个；
//! - 归一化失败（相对路径等）的候选不进入映射；
//! - 状态（禁用 / 有效 / 缺失 / 不可用）与内容一起构成**确定性 revision**：
//!   任何变化都必须让依赖它的解析缓存与查询复用键失效。

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use super::project_path::normalize_project_path;

/// 映射状态；与内容摘要一起构成 revision。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingState {
    /// 显式禁用（自定义来源且无配置关联）——不读取任何账户配置。
    Disabled,
    /// 已加载（可能为空表）。
    Loaded,
    /// 配置文件缺失。
    Missing,
    /// 读取失败或 JSON 结构不可用（损坏）。
    Unusable,
}

impl MappingState {
    fn tag(self) -> &'static str {
        match self {
            MappingState::Disabled => "disabled",
            MappingState::Loaded => "loaded",
            MappingState::Missing => "missing",
            MappingState::Unusable => "unusable",
        }
    }
}

/// 一次加载得到的不可变映射（slug → 归一化项目身份）。
#[derive(Debug)]
pub struct ProjectMapping {
    by_slug: HashMap<String, String>,
    revision: String,
    state: MappingState,
    entries: usize,
    conflicts: Vec<String>,
}

impl ProjectMapping {
    /// 显式禁用：不读取任何配置，slug 保持独立身份。
    pub fn disabled() -> Self {
        Self {
            by_slug: HashMap::new(),
            revision: compute_revision(MappingState::Disabled, &HashMap::new()),
            state: MappingState::Disabled,
            entries: 0,
            conflicts: Vec::new(),
        }
    }

    /// 从指定配置文件加载（调用方负责给出路径；不存在/损坏都不报错）。
    pub fn load_from(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Self::empty(MappingState::Missing);
            }
            Err(_) => return Self::empty(MappingState::Unusable),
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return Self::empty(MappingState::Unusable),
        };
        let Some(projects) = value.get("projects").and_then(serde_json::Value::as_object) else {
            return Self::empty(MappingState::Unusable);
        };
        // slug → 归一化身份；None = 该 slug 候选互相矛盾（冲突，不入表）。
        let mut candidates: HashMap<String, Option<String>> = HashMap::new();
        let mut conflicts = Vec::new();
        for raw in projects.keys() {
            let Some(identity) = normalize_project_path(raw) else {
                continue;
            };
            let slug = slug_for_path(raw);
            match candidates.entry(slug) {
                Entry::Vacant(e) => {
                    e.insert(Some(identity));
                }
                Entry::Occupied(mut e) => {
                    if e.get().as_deref() != Some(identity.as_str()) {
                        if e.get().is_some() {
                            conflicts.push(e.key().clone());
                        }
                        *e.get_mut() = None;
                    }
                }
            }
        }
        let by_slug: HashMap<String, String> = candidates
            .into_iter()
            .filter_map(|(slug, identity)| identity.map(|i| (slug, i)))
            .collect();
        let entries = by_slug.len();
        conflicts.sort();
        Self {
            revision: compute_revision(MappingState::Loaded, &by_slug),
            by_slug,
            state: MappingState::Loaded,
            entries,
            conflicts,
        }
    }

    /// 进程内按指纹缓存（size + mtime）加载：同一采集批次只实际解析一次，
    /// 配置内容变化（指纹变）即重新加载——不是"启动时永久缓存"。
    pub fn load_cached(path: &Path) -> Arc<Self> {
        let fingerprint = file_fingerprint(path);
        let key = path.to_path_buf();
        {
            let guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((cached, mapping)) = guard.get(&key)
                && *cached == fingerprint
            {
                return mapping.clone();
            }
        }
        let loaded = Arc::new(Self::load_from(path));
        // 第三轮审查：**读取失败不进缓存**——配置被独占/短暂不可读时得到的
        // 失败状态若被缓存（指纹是文件本身的 size+mtime，不会变化），恢复可读
        // 后仍会沿用失败映射、保留 slug 身份，直到文件指纹变化或进程重启。
        if loaded.state() != MappingState::Unusable {
            let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            guard.insert(key, (fingerprint, loaded.clone()));
        }
        loaded
    }

    fn empty(state: MappingState) -> Self {
        Self {
            by_slug: HashMap::new(),
            revision: compute_revision(state, &HashMap::new()),
            state,
            entries: 0,
            conflicts: Vec::new(),
        }
    }

    /// slug 命中唯一正向映射 → 归一化项目身份；冲突/未命中/禁用 → None。
    pub fn resolve(&self, slug: &str) -> Option<&str> {
        self.by_slug.get(slug).map(String::as_str)
    }

    /// 确定性修订：状态 + 全部映射内容的摘要。
    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn state(&self) -> MappingState {
        self.state
    }

    pub fn entries(&self) -> usize {
        self.entries
    }

    /// 冲突 slug（同一 slug 对应多个归一化后仍不同的路径）——保持独立身份。
    pub fn conflicts(&self) -> &[String] {
        &self.conflicts
    }
}

/// Claude Code 的项目目录编码：每个非 ASCII 字母数字字符 → `-`。
pub fn slug_for_path(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn compute_revision(state: MappingState, by_slug: &HashMap<String, String>) -> String {
    let mut pairs: Vec<(&String, &String)> = by_slug.iter().collect();
    pairs.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    state.tag().hash(&mut hasher);
    for (slug, identity) in pairs {
        slug.hash(&mut hasher);
        identity.hash(&mut hasher);
    }
    format!("{}-{:016x}", state.tag(), hasher.finish())
}

/// 文件指纹：`None` = 文件不存在/不可读（缺失与可读必须区分）。
/// 时间用纳秒精度——毫秒截断会让"同一毫秒内的两次改写"看起来没变。
fn file_fingerprint(path: &Path) -> Option<(u64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let nanos = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((meta.len(), i64::try_from(nanos.as_nanos()).ok()?))
}

type CacheEntry = (Option<(u64, i64)>, Arc<ProjectMapping>);
static CACHE: LazyLock<Mutex<HashMap<PathBuf, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-map-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn slug_encoding_matches_local_layout() {
        assert_eq!(
            slug_for_path(r"C:\Users\admin\Desktop\项目\ciweimao"),
            "C--Users-admin-Desktop----ciweimao"
        );
        assert_eq!(slug_for_path("/home/u/.config"), "-home-u--config");
    }

    #[test]
    fn forward_mapping_is_unique_per_slug() {
        let dir = tmp("unique");
        let f = write(
            &dir,
            "claude.json",
            r#"{"projects":{"C:\\work\\alpha":{},"relative\\x":{}}}"#,
        );
        let m = ProjectMapping::load_from(&f);
        assert_eq!(m.state(), MappingState::Loaded);
        assert_eq!(m.entries(), 1, "归一化失败的候选不进映射");
        assert_eq!(m.resolve("C--work-alpha"), Some("C:/work/alpha"));
        assert_eq!(m.resolve("-relative-x"), None);
    }

    #[test]
    fn colliding_candidates_are_dropped_not_first_wins() {
        let dir = tmp("collide");
        // `.` 与 `-` 都编码为 `-`：两个不同路径落到同一 slug。
        let f = write(
            &dir,
            "claude.json",
            r#"{"projects":{"C:\\work\\a.b":{},"C:\\work\\a-b":{}}}"#,
        );
        let m = ProjectMapping::load_from(&f);
        assert_eq!(m.resolve("C--work-a-b"), None);
        assert_eq!(m.conflicts(), ["C--work-a-b"]);
    }

    #[test]
    fn state_and_content_make_revision_deterministic() {
        let dir = tmp("revision");
        let missing = ProjectMapping::load_from(&dir.join("nope.json"));
        assert_eq!(missing.state(), MappingState::Missing);
        let bad = ProjectMapping::load_from(&write(&dir, "bad.json", "not json"));
        assert_eq!(bad.state(), MappingState::Unusable);
        assert_ne!(
            missing.revision(),
            bad.revision(),
            "状态变化必须改变 revision"
        );
        let ok = ProjectMapping::load_from(&write(
            &dir,
            "ok.json",
            r#"{"projects":{"C:\\work\\alpha":{}}}"#,
        ));
        let same = ProjectMapping::load_from(&dir.join("ok.json"));
        assert_eq!(ok.revision(), same.revision(), "同内容 → 同 revision");
        assert_ne!(ok.revision(), ProjectMapping::disabled().revision());
    }
}
