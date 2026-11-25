//! 模型展示名的独立查询：规则在此，名称数据在 `data/model_display_catalog.json`。
//!
//! 与报价索引完全解耦——无价格快照、旧 v4 快照（无 canonical_model_id）、同步
//! 失败保留旧快照时仍能解析已收录的规范名称。名称不参与分组键、下钻、覆盖或
//! 计价；只经 [`crate::pricing::Pricing::display_name_for`] 供聚合标签消费。

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};

use serde::Deserialize;

use crate::model_identity::ModelIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayNameReason {
    ResolvedCatalog,
    ResolvedSnapshot,
    EmptyKey,
    NoIdentityEvidence,
    AmbiguousIdentity,
    ConflictingNames,
    NoTrustedName,
}

impl DisplayNameReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResolvedCatalog => "resolved_catalog",
            Self::ResolvedSnapshot => "resolved_snapshot",
            Self::EmptyKey => "empty_model_key",
            Self::NoIdentityEvidence => "no_identity_evidence",
            Self::AmbiguousIdentity => "ambiguous_identity",
            Self::ConflictingNames => "conflicting_names",
            Self::NoTrustedName => "no_trusted_name",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayNameResolution {
    pub name: Option<String>,
    pub canonical_id: Option<String>,
    pub source: &'static str,
    pub reason: DisplayNameReason,
}

impl DisplayNameResolution {
    fn unresolved(reason: DisplayNameReason) -> Self {
        Self {
            name: None,
            canonical_id: None,
            source: "none",
            reason,
        }
    }
}

#[derive(Debug, Deserialize)]
struct EmbeddedCatalog {
    models: Vec<EmbeddedModel>,
    aliases: Vec<EmbeddedAlias>,
}

#[derive(Debug, Deserialize)]
struct EmbeddedModel {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddedAlias {
    id: String,
    canonical_id: String,
}

/// 公开目录索引只构建一次并由所有查询会话共享，不随每次冻结快照复制。
#[derive(Debug, Default)]
struct EmbeddedIndex {
    canonical: HashMap<String, String>,
    canonical_raw: HashMap<String, String>,
    canonical_leaf: HashMap<String, BTreeSet<String>>,
    aliases: HashMap<String, String>,
    alias_leaf: HashMap<String, BTreeSet<String>>,
}

static EMBEDDED: OnceLock<Arc<EmbeddedIndex>> = OnceLock::new();

impl EmbeddedIndex {
    fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let data: EmbeddedCatalog = serde_json::from_str(json)?;
        let mut index = Self::default();
        for model in &data.models {
            let Some(key) = identity_key(&model.id) else {
                continue;
            };
            index.canonical.insert(key.clone(), model.name.clone());
            index.canonical_raw.insert(key.clone(), model.id.clone());
            let leaf = ModelIdentity::parse(&model.id).leaf_key().to_string();
            index.canonical_leaf.entry(leaf).or_default().insert(key);
        }
        for alias in &data.aliases {
            let (Some(from), Some(to)) =
                (identity_key(&alias.id), identity_key(&alias.canonical_id))
            else {
                continue;
            };
            index.aliases.insert(from.clone(), to.clone());
            let leaf = ModelIdentity::parse(&alias.id).leaf_key().to_string();
            index.alias_leaf.entry(leaf).or_default().insert(to);
        }
        Ok(index)
    }
}

fn embedded() -> &'static Arc<EmbeddedIndex> {
    EMBEDDED.get_or_init(|| {
        Arc::new(
            EmbeddedIndex::from_json(include_str!("../data/model_display_catalog.json"))
                .expect("嵌入的展示名目录必须可解析"),
        )
    })
}

#[derive(Debug, Clone)]
struct SnapshotName {
    display: String,
    name: String,
}

fn usable(name: &str) -> bool {
    !name.trim().is_empty()
}

/// name 是条目 ID 的原样抄写时只作兜底，不能盖掉正式名称。
fn is_id_copy(name: &str, display: &str) -> bool {
    let name = name.trim();
    let leaf = display.rsplit('/').next().unwrap_or(display);
    if name != leaf && name != display {
        return false;
    }
    name.chars().all(|c| !c.is_uppercase())
        && (name.contains('-') || name.contains('_') || name.contains('.'))
}

fn quality(name: &str, display: &str) -> u8 {
    if is_id_copy(name, display) { 1 } else { 2 }
}

fn identity_key(raw: &str) -> Option<String> {
    let id = ModelIdentity::parse(raw);
    (!id.is_empty_key()).then(|| id.identity_key())
}

#[derive(Debug, Clone)]
pub struct NameCatalog {
    /// 全局共享的公开规范索引（按查询会话不重复记账）。
    embedded: Arc<EmbeddedIndex>,
    /// 本次快照中条目 ID 的完整身份键（含无价名称条目）。
    snapshot_ids: BTreeSet<String>,
    /// 本次快照中条目自身完整键 → 显式 canonical 目标；冲突时丢弃。
    claims: HashMap<String, String>,
    snapshot_names: HashMap<String, Vec<SnapshotName>>,
}

impl Default for NameCatalog {
    fn default() -> Self {
        Self::offline()
    }
}

impl NameCatalog {
    /// 完全离线：只使用随程序发布的公开规范目录。
    pub fn offline() -> Self {
        Self {
            embedded: embedded().clone(),
            snapshot_ids: BTreeSet::new(),
            claims: HashMap::new(),
            snapshot_names: HashMap::new(),
        }
    }

    /// 合并本次冻结的 models.dev 快照（价格层之外的名称元数据）。
    pub fn from_snapshot(snapshot: Option<&crate::modelsdev::Snapshot>) -> Self {
        let mut this = Self::offline();
        let Some(snapshot) = snapshot else {
            return this;
        };
        let mut claim_candidates: HashMap<String, Option<String>> = HashMap::new();
        for (id, name, canonical) in snapshot
            .entries
            .iter()
            .map(|e| (&e.id, e.name.as_deref(), e.canonical_model_id.as_deref()))
            .chain(
                snapshot
                    .names
                    .iter()
                    .map(|e| (&e.id, e.name.as_deref(), e.canonical_model_id.as_deref())),
            )
        {
            let Some(key) = identity_key(id) else {
                continue;
            };
            this.snapshot_ids.insert(key.clone());
            if let Some(name) = name.filter(|n| usable(n)) {
                this.snapshot_names
                    .entry(key.clone())
                    .or_default()
                    .push(SnapshotName {
                        display: id.clone(),
                        name: name.to_string(),
                    });
            }
            let Some(target) = canonical.and_then(identity_key) else {
                continue;
            };
            let claim = claim_candidates.entry(key).or_insert(Some(target.clone()));
            if claim.as_deref() != Some(target.as_str()) {
                *claim = None;
            }
        }
        this.claims = claim_candidates
            .into_iter()
            .filter_map(|(key, target)| target.map(|t| (key, t)))
            .collect();
        this
    }

    /// 裸 ID 的身份只能由显式规范关系或唯一目录身份确认；名称近似与末段相同
    /// 都不是身份证据。
    fn bare_target(&self, leaf: &str) -> Result<String, DisplayNameReason> {
        if let Some(targets) = self.embedded.canonical_leaf.get(leaf) {
            if targets.len() == 1 {
                return Ok(targets.iter().next().unwrap().clone());
            }
            if targets.len() > 1 {
                return Err(DisplayNameReason::AmbiguousIdentity);
            }
        }
        if let Some(targets) = self.embedded.alias_leaf.get(leaf) {
            if targets.len() == 1 {
                return Ok(targets.iter().next().unwrap().clone());
            }
            if targets.len() > 1 {
                return Err(DisplayNameReason::AmbiguousIdentity);
            }
        }
        let mut claimed: BTreeSet<String> = BTreeSet::new();
        let mut raw: BTreeSet<String> = BTreeSet::new();
        for key in &self.snapshot_ids {
            if ModelIdentity::parse(key).leaf_key() != leaf {
                continue;
            }
            if let Some(target) = self.claims.get(key) {
                claimed.insert(target.clone());
            }
            raw.insert(key.clone());
        }
        if claimed.len() == 1 {
            return Ok(claimed.iter().next().unwrap().clone());
        }
        if claimed.len() > 1 {
            return Err(DisplayNameReason::AmbiguousIdentity);
        }
        match raw.len() {
            1 => Ok(raw.iter().next().unwrap().clone()),
            0 => Err(DisplayNameReason::NoIdentityEvidence),
            _ => Err(DisplayNameReason::AmbiguousIdentity),
        }
    }

    fn choose_name(&self, target: &str) -> DisplayNameResolution {
        if let Some(name) = self.embedded.canonical.get(target) {
            return DisplayNameResolution {
                name: Some(name.clone()),
                canonical_id: Some(
                    self.embedded
                        .canonical_raw
                        .get(target)
                        .cloned()
                        .unwrap_or_else(|| target.to_string()),
                ),
                source: "models.json",
                reason: DisplayNameReason::ResolvedCatalog,
            };
        }
        let mut official: Vec<&SnapshotName> = Vec::new();
        let mut channels: Vec<&SnapshotName> = Vec::new();
        for (key, names) in &self.snapshot_names {
            for entry in names {
                if key == target {
                    official.push(entry);
                } else if self.claims.get(key).is_some_and(|t| t == target) {
                    channels.push(entry);
                }
            }
        }
        for (pool, level) in [
            (&official, 2u8),
            (&channels, 2),
            (&official, 1),
            (&channels, 1),
        ] {
            let eligible: Vec<&SnapshotName> = pool
                .iter()
                .copied()
                .filter(|e| quality(&e.name, &e.display) == level)
                .collect();
            if eligible.is_empty() {
                continue;
            }
            let distinct: BTreeSet<&str> = eligible.iter().map(|e| e.name.as_str()).collect();
            if distinct.len() > 1 {
                return DisplayNameResolution::unresolved(DisplayNameReason::ConflictingNames);
            }
            let best = eligible
                .iter()
                .min_by(|a, b| (&a.display, &a.name).cmp(&(&b.display, &b.name)))
                .unwrap();
            return DisplayNameResolution {
                name: Some(best.name.clone()),
                canonical_id: Some(target.to_string()),
                source: "models.dev-snapshot",
                reason: DisplayNameReason::ResolvedSnapshot,
            };
        }
        let mut missing = DisplayNameResolution::unresolved(DisplayNameReason::NoTrustedName);
        missing.canonical_id = Some(target.to_string());
        missing
    }

    /// 解析展示名；`confirmed_base` 是统计层已核实的基础完整 ID（离线也有效）。
    pub fn resolve(&self, model: &str, confirmed_base: Option<&str>) -> DisplayNameResolution {
        let id = ModelIdentity::parse(model);
        if id.is_empty_key() {
            return DisplayNameResolution::unresolved(DisplayNameReason::EmptyKey);
        }
        let structural = id.identity_key();
        let target = if let Some(base) = confirmed_base.and_then(identity_key) {
            base
        } else if self.embedded.canonical.contains_key(&structural) {
            // 完整 ID 自身的规范名优先于任何渠道别名或快照 canonical 关系；
            // 这些关系只补充没有自身规范名的条目。
            structural.clone()
        } else if let Some(target) = self.embedded.aliases.get(&structural) {
            target.clone()
        } else if let Some(target) = self.claims.get(&structural) {
            target.clone()
        } else if id.namespace().is_some() {
            structural.clone()
        } else {
            let leaf = id.leaf_key();
            match self.bare_target(leaf) {
                Ok(target) => target,
                Err(reason) => return DisplayNameResolution::unresolved(reason),
            }
        };
        let resolved = self.choose_name(&target);
        if resolved.name.is_none() {
            // 带命名空间但自身没有合格名称时不得借同末段的其它发布方。
            if id.namespace().is_some() && confirmed_base.is_none() {
                return DisplayNameResolution::unresolved(DisplayNameReason::NoTrustedName);
            }
        }
        resolved
    }

    /// 查询快照的增量记账：公开规范索引是全局共享的静态数据，不按会话重复计。
    pub(crate) fn retained_bytes(&self) -> Result<usize, String> {
        use crate::report::retained::{
            ByteCount, MAP_BUCKET_OVERHEAD, capacity_bytes, string_bytes,
        };
        let mut n = ByteCount::default();
        n.add(capacity_bytes(
            self.claims.capacity(),
            std::mem::size_of::<(String, String)>() + MAP_BUCKET_OVERHEAD,
        )?)?;
        for (k, v) in &self.claims {
            n.add(string_bytes(k)?)?;
            n.add(string_bytes(v)?)?;
        }
        n.add(capacity_bytes(
            self.snapshot_ids.len(),
            std::mem::size_of::<String>(),
        )?)?;
        for key in &self.snapshot_ids {
            n.add(string_bytes(key)?)?;
        }
        n.add(capacity_bytes(
            self.snapshot_names.capacity(),
            std::mem::size_of::<(String, Vec<SnapshotName>)>() + MAP_BUCKET_OVERHEAD,
        )?)?;
        for (k, names) in &self.snapshot_names {
            n.add(string_bytes(k)?)?;
            n.add(crate::report::retained::vec_bytes(names)?)?;
            for name in names {
                n.add(string_bytes(&name.display)?)?;
                n.add(string_bytes(&name.name)?)?;
            }
        }
        Ok(n.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_from(json: &str) -> NameCatalog {
        NameCatalog {
            embedded: Arc::new(EmbeddedIndex::from_json(json).expect("合成目录必须可解析")),
            snapshot_ids: BTreeSet::new(),
            claims: HashMap::new(),
            snapshot_names: HashMap::new(),
        }
    }

    /// 生成器保留末段相同的显式别名后，完整 ID 与裸 ID 都能离线使用该关系。
    #[test]
    fn same_leaf_explicit_alias_resolves_offline() {
        let forward = r#"{
            "models": [
                {"id": "publisher/demo-v2", "name": "Demo V2"},
                {"id": "publisher/demo-v3", "name": "Demo V3"}
            ],
            "aliases": [
                {"id": "relay/demo-v2", "canonical_id": "publisher/demo-v2"},
                {"id": "relay/demo-v3", "canonical_id": "publisher/demo-v3"}
            ]
        }"#;
        let reversed = r#"{
            "aliases": [
                {"id": "relay/demo-v3", "canonical_id": "publisher/demo-v3"},
                {"id": "relay/demo-v2", "canonical_id": "publisher/demo-v2"}
            ],
            "models": [
                {"id": "publisher/demo-v3", "name": "Demo V3"},
                {"id": "publisher/demo-v2", "name": "Demo V2"}
            ]
        }"#;
        for json in [forward, reversed] {
            let catalog = catalog_from(json);
            let relay = catalog.resolve("relay/demo-v2", None);
            assert_eq!(relay.name.as_deref(), Some("Demo V2"), "{relay:?}");
            assert_eq!(
                relay.canonical_id.as_deref(),
                Some("publisher/demo-v2"),
                "{relay:?}"
            );
            assert_eq!(
                catalog.resolve("publisher/demo-v2", None).name.as_deref(),
                Some("Demo V2")
            );
            assert_eq!(
                catalog.resolve("demo-v2", None).name.as_deref(),
                Some("Demo V2")
            );
            assert_eq!(
                catalog.resolve("relay/demo-v3", None).name.as_deref(),
                Some("Demo V3")
            );
        }
    }

    /// 两个发布方确有不同型号：带前缀的完整 ID 各用自己的名称，无法确认身份
    /// 的裸 ID 保留原文；目录乱序不改变结果。
    #[test]
    fn same_leaf_publishers_keep_own_names_and_bare_stays_raw() {
        let forward = r#"{
            "models": [
                {"id": "author-a/demo-x", "name": "Demo A One"},
                {"id": "author-b/demo-x", "name": "Demo B Two"}
            ],
            "aliases": [{"id": "relay/demo-x", "canonical_id": "author-a/demo-x"}]
        }"#;
        let reversed = r#"{
            "aliases": [{"id": "relay/demo-x", "canonical_id": "author-a/demo-x"}],
            "models": [
                {"id": "author-b/demo-x", "name": "Demo B Two"},
                {"id": "author-a/demo-x", "name": "Demo A One"}
            ]
        }"#;
        for json in [forward, reversed] {
            let catalog = catalog_from(json);
            assert_eq!(
                catalog.resolve("author-a/demo-x", None).name.as_deref(),
                Some("Demo A One")
            );
            assert_eq!(
                catalog.resolve("author-b/demo-x", None).name.as_deref(),
                Some("Demo B Two")
            );
            assert_eq!(
                catalog.resolve("relay/demo-x", None).name.as_deref(),
                Some("Demo A One")
            );
            let bare = catalog.resolve("demo-x", None);
            assert_eq!(bare.name, None, "{bare:?}");
            assert_eq!(bare.reason, DisplayNameReason::AmbiguousIdentity);
        }
    }

    /// 完整 ID 自身的规范名优先于同末段别名（即使别名来源本身也是规范模型）。
    #[test]
    fn own_canonical_name_beats_same_leaf_alias() {
        let forward = r#"{
            "models": [
                {"id": "publisher/demo-v2", "name": "Demo V2"},
                {"id": "relay/demo-v2", "name": "Relay Demo V2"}
            ],
            "aliases": [{"id": "relay/demo-v2", "canonical_id": "publisher/demo-v2"}]
        }"#;
        let reversed = r#"{
            "aliases": [{"id": "relay/demo-v2", "canonical_id": "publisher/demo-v2"}],
            "models": [
                {"id": "relay/demo-v2", "name": "Relay Demo V2"},
                {"id": "publisher/demo-v2", "name": "Demo V2"}
            ]
        }"#;
        for json in [forward, reversed] {
            let catalog = catalog_from(json);
            assert_eq!(
                catalog.resolve("publisher/demo-v2", None).name.as_deref(),
                Some("Demo V2")
            );
            assert_eq!(
                catalog.resolve("relay/demo-v2", None).name.as_deref(),
                Some("Relay Demo V2")
            );
            let bare = catalog.resolve("demo-v2", None);
            assert_eq!(bare.name, None, "{bare:?}");
            assert_eq!(bare.reason, DisplayNameReason::AmbiguousIdentity);
        }
    }

    #[test]
    fn embedded_catalog_has_verified_names() {
        let catalog = NameCatalog::offline();
        for (id, name) in [
            ("claude-opus-4-8", "Claude Opus 4.8"),
            ("doubao-seed-2-0-pro-260215", "Seed 2.0 Pro"),
            ("deepseek-v4-flash-0731", "DeepSeek V4 Flash 0731"),
        ] {
            let r = catalog.resolve(id, None);
            assert_eq!(r.reason, DisplayNameReason::ResolvedCatalog, "{id}");
            assert_eq!(r.name.as_deref(), Some(name), "{id}");
        }
        assert_eq!(
            catalog.resolve("gpt-6-astra-cc-format", None).reason,
            DisplayNameReason::NoIdentityEvidence
        );
    }
}
