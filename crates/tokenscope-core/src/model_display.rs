//! 模型展示名的独立查询：规则在此，随包名称数据在 `data/model_display_catalog.json`，
//! 运行时同时合并 models.dev 与 OpenRouter 同步快照中的名称元数据。
//!
//! 与报价索引完全解耦——无价格快照、旧 v4 快照（无 canonical_model_id）、无报价条目、
//! 同步失败保留旧快照时仍能按双源规则解析可信名称，并严格区分「已查且无匹配」与
//! 「来源不可用」。名称不参与分组键、下钻、覆盖或计价；只经
//! [`crate::pricing::Pricing::display_name_for`] 供聚合标签消费。

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};

use serde::Deserialize;

use crate::model_identity::ModelIdentity;

/// 单个名称来源快照的加载可用性（由价格加载层提供，与报价索引解耦）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SnapshotAvailability {
    /// 快照已成功读取并解析。
    Available,
    /// 快照未配置、文件不存在或处于离线模式。
    #[default]
    NotSynced,
    /// 快照读取失败（I/O / UTF-8）或 JSON 解析失败。
    LoadFailed,
}

impl SnapshotAvailability {
    fn no_match_status(self) -> SourceLookupStatus {
        match self {
            Self::Available => SourceLookupStatus::CheckedNoMatch,
            Self::NotSynced => SourceLookupStatus::UnavailableNotSynced,
            Self::LoadFailed => SourceLookupStatus::UnavailableLoadFailed,
        }
    }
}

/// 单个名称来源在本次查询中的状态：严格区分「已查且无匹配」与「来源不可用」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLookupStatus {
    /// 该来源已查并命中可信名称（或同型号确认命中）。
    Matched,
    /// 该来源已加载并完成检查，但无匹配或无可用可信名称。
    CheckedNoMatch,
    /// 该来源已加载，但在该查询上存在内部歧义、名称冲突或跨源身份冲突。
    AmbiguousOrConflict,
    /// 该来源未同步 / 离线不可用。
    UnavailableNotSynced,
    /// 该来源快照读取或解析失败，不可用。
    UnavailableLoadFailed,
}

impl SourceLookupStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::CheckedNoMatch => "checked_no_match",
            Self::AmbiguousOrConflict => "ambiguous_or_conflict",
            Self::UnavailableNotSynced => "unavailable_not_synced",
            Self::UnavailableLoadFailed => "unavailable_load_failed",
        }
    }

    pub fn is_unavailable(self) -> bool {
        matches!(
            self,
            Self::UnavailableNotSynced | Self::UnavailableLoadFailed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayNameReason {
    /// 命中随包公开规范目录（models.dev models.json）。
    ResolvedCatalog,
    /// 命中 models.dev 同步快照。
    ResolvedSnapshot,
    /// 命中 OpenRouter 同步快照。
    ResolvedOpenRouter,
    EmptyKey,
    /// 双源均已检查，均无身份证据。
    NoIdentityEvidence,
    /// 单来源内部身份歧义（如裸 ID 命中多个不同型号）。
    AmbiguousIdentity,
    /// models.dev 与 OpenRouter 指向不同型号的真实跨源冲突。
    ConflictingSources,
    /// 同一目标身份下存在互斥名称。
    ConflictingNames,
    /// 双源均已检查，已定位身份但无可用可信名称。
    NoTrustedName,
    /// 至少一个来源不可用（未同步、离线或加载失败），且可用来源未命中；
    /// 区别于双源均已检查后的确认无匹配。
    SourceUnavailable,
}

impl DisplayNameReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResolvedCatalog => "resolved_catalog",
            Self::ResolvedSnapshot => "resolved_snapshot",
            Self::ResolvedOpenRouter => "resolved_openrouter",
            Self::EmptyKey => "empty_model_key",
            Self::NoIdentityEvidence => "no_identity_evidence",
            Self::AmbiguousIdentity => "ambiguous_identity",
            Self::ConflictingSources => "conflicting_sources",
            Self::ConflictingNames => "conflicting_names",
            Self::NoTrustedName => "no_trusted_name",
            Self::SourceUnavailable => "source_unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayNameResolution {
    pub name: Option<String>,
    pub canonical_id: Option<String>,
    pub source: &'static str,
    pub reason: DisplayNameReason,
    pub modelsdev_status: SourceLookupStatus,
    pub openrouter_status: SourceLookupStatus,
}

impl DisplayNameResolution {
    fn unresolved(
        reason: DisplayNameReason,
        modelsdev_status: SourceLookupStatus,
        openrouter_status: SourceLookupStatus,
    ) -> Self {
        Self {
            name: None,
            canonical_id: None,
            source: "none",
            reason,
            modelsdev_status,
            openrouter_status,
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
enum CandidateName {
    Catalog {
        name: String,
        canonical_id: String,
    },
    Snapshot {
        name: String,
        canonical_id: String,
        quality: u8,
        source: &'static str,
        reason: DisplayNameReason,
    },
    ConflictingNames,
    None,
}

enum OrBareMatch {
    Unavailable(SnapshotAvailability),
    NoMatch,
    Ambiguous,
    Unique { target: String },
}

#[derive(Debug, Clone)]
pub struct NameCatalog {
    /// 全局共享的公开规范索引（按查询会话不重复记账）。
    embedded: Arc<EmbeddedIndex>,
    modelsdev_availability: SnapshotAvailability,
    openrouter_availability: SnapshotAvailability,
    /// 本次 models.dev 快照中条目 ID 的完整身份键（含无价名称条目）。
    snapshot_ids: BTreeSet<String>,
    /// 本次 models.dev 快照中条目自身完整键 → 显式 canonical 目标；冲突时丢弃。
    claims: HashMap<String, String>,
    snapshot_names: HashMap<String, Vec<SnapshotName>>,
    /// 本次 OpenRouter 快照中条目 ID 的完整身份键（含无价条目）。
    openrouter_ids: BTreeSet<String>,
    openrouter_names: HashMap<String, Vec<SnapshotName>>,
}

impl Default for NameCatalog {
    fn default() -> Self {
        Self::offline()
    }
}

impl NameCatalog {
    /// 完全离线：只使用随程序发布的公开规范目录，两路快照均记为未同步。
    pub fn offline() -> Self {
        Self {
            embedded: embedded().clone(),
            modelsdev_availability: SnapshotAvailability::NotSynced,
            openrouter_availability: SnapshotAvailability::NotSynced,
            snapshot_ids: BTreeSet::new(),
            claims: HashMap::new(),
            snapshot_names: HashMap::new(),
            openrouter_ids: BTreeSet::new(),
            openrouter_names: HashMap::new(),
        }
    }

    /// 测试用：替换随包公开目录 JSON，保留已有双源快照与可用性状态。
    pub fn with_embedded_json_for_tests(&self, json: &str) -> Self {
        Self {
            embedded: Arc::new(EmbeddedIndex::from_json(json).expect("合成展示名目录必须可解析")),
            modelsdev_availability: self.modelsdev_availability,
            openrouter_availability: self.openrouter_availability,
            snapshot_ids: self.snapshot_ids.clone(),
            claims: self.claims.clone(),
            snapshot_names: self.snapshot_names.clone(),
            openrouter_ids: self.openrouter_ids.clone(),
            openrouter_names: self.openrouter_names.clone(),
        }
    }

    /// 兼容入口：仅合并 models.dev 快照（OpenRouter 记为未同步）。
    pub fn from_snapshot(snapshot: Option<&crate::modelsdev::Snapshot>) -> Self {
        let md_avail = if snapshot.is_some() {
            SnapshotAvailability::Available
        } else {
            SnapshotAvailability::NotSynced
        };
        Self::from_snapshots(snapshot, md_avail, None, SnapshotAvailability::NotSynced)
    }

    /// 合并本次冻结的 models.dev 与 OpenRouter 快照（价格层之外的名称元数据）。
    pub fn from_snapshots(
        modelsdev: Option<&crate::modelsdev::Snapshot>,
        modelsdev_availability: SnapshotAvailability,
        openrouter: Option<&crate::openrouter::Snapshot>,
        openrouter_availability: SnapshotAvailability,
    ) -> Self {
        let mut this = Self::offline();
        this.modelsdev_availability = modelsdev_availability;
        this.openrouter_availability = openrouter_availability;

        if let Some(snapshot) = modelsdev {
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
                if let Some(name) = name.map(str::trim).filter(|n| usable(n)) {
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
        }

        if let Some(snapshot) = openrouter {
            for (id, name) in snapshot
                .entries
                .iter()
                .map(|e| (&e.id, e.name.as_deref()))
                .chain(snapshot.names.iter().map(|e| (&e.id, e.name.as_deref())))
            {
                let Some(key) = identity_key(id) else {
                    continue;
                };
                this.openrouter_ids.insert(key.clone());
                if let Some(name) = name.map(str::trim).filter(|n| usable(n)) {
                    this.openrouter_names
                        .entry(key)
                        .or_default()
                        .push(SnapshotName {
                            display: id.clone(),
                            name: name.to_string(),
                        });
                }
            }
        }

        this
    }

    /// OpenRouter 条目经 models.dev 显式别名（如 `openrouter/z-ai/glm-5.2 -> zhipuai/glm-5.2`）
    /// 确认的规范目标；无显式别名时返回 None（绝不按末段相似猜关系）。
    fn openrouter_explicit_canonical(&self, or_key: &str) -> Option<String> {
        let prefixed = format!("openrouter/{or_key}");
        self.embedded
            .aliases
            .get(or_key)
            .or_else(|| self.embedded.aliases.get(&prefixed))
            .or_else(|| self.claims.get(or_key))
            .or_else(|| self.claims.get(&prefixed))
            .cloned()
    }

    fn pick_pool_name_at_level(
        pool: &[&SnapshotName],
        level: u8,
        canonical_id: &str,
        source: &'static str,
        reason: DisplayNameReason,
    ) -> CandidateName {
        let eligible: Vec<&SnapshotName> = pool
            .iter()
            .copied()
            .filter(|e| quality(&e.name, &e.display) == level)
            .collect();
        if eligible.is_empty() {
            return CandidateName::None;
        }
        let distinct: BTreeSet<&str> = eligible.iter().map(|e| e.name.as_str()).collect();
        if distinct.len() > 1 {
            return CandidateName::ConflictingNames;
        }
        let best = eligible
            .iter()
            .min_by(|a, b| (&a.display, &a.name).cmp(&(&b.display, &b.name)))
            .unwrap();
        CandidateName::Snapshot {
            name: best.name.clone(),
            canonical_id: canonical_id.to_string(),
            quality: level,
            source,
            reason,
        }
    }

    /// 仅从 models.dev（随包规范目录 + models.dev 快照）为指定目标取名。
    fn choose_modelsdev_name(&self, target: &str) -> CandidateName {
        if let Some(name) = self.embedded.canonical.get(target) {
            return CandidateName::Catalog {
                name: name.clone(),
                canonical_id: self
                    .embedded
                    .canonical_raw
                    .get(target)
                    .cloned()
                    .unwrap_or_else(|| target.to_string()),
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
                return CandidateName::ConflictingNames;
            }
            let best = eligible
                .iter()
                .min_by(|a, b| (&a.display, &a.name).cmp(&(&b.display, &b.name)))
                .unwrap();
            return CandidateName::Snapshot {
                name: best.name.clone(),
                canonical_id: target.to_string(),
                quality: level,
                source: "models.dev-snapshot",
                reason: DisplayNameReason::ResolvedSnapshot,
            };
        }
        CandidateName::None
    }

    /// 从 OpenRouter 快照为指定目标取名：
    /// - 先查直接查询命中的精确键（若有），再查目标精确键，最后查已确认指向同一目标的别名；
    /// - 精确条目无可用名称（或仅为 ID 抄写）时继续查已确认别名，绝不按同末段借名；
    /// - 正式名（quality=2）优先于 ID 抄写（quality=1），与 `choose_modelsdev_name` 对称。
    fn choose_openrouter_name_for_target(
        &self,
        direct_key: Option<&str>,
        target: &str,
    ) -> CandidateName {
        if self.openrouter_availability != SnapshotAvailability::Available {
            return CandidateName::None;
        }
        let mut direct: Vec<&SnapshotName> = Vec::new();
        if let Some(dk) = direct_key
            && dk != target
            && let Some(entries) = self.openrouter_names.get(dk)
        {
            direct.extend(entries.iter());
        }
        let mut exact: Vec<&SnapshotName> = Vec::new();
        if let Some(entries) = self.openrouter_names.get(target) {
            exact.extend(entries.iter());
        }
        let mut aliased: Vec<&SnapshotName> = Vec::new();
        for (key, entries) in &self.openrouter_names {
            if key == target || Some(key.as_str()) == direct_key || key.starts_with('~') {
                continue;
            }
            if self
                .openrouter_explicit_canonical(key)
                .is_some_and(|canon| canon == target)
            {
                aliased.extend(entries.iter());
            }
        }
        for level in [2u8, 1] {
            for pool in [&direct, &exact, &aliased] {
                let candidate = Self::pick_pool_name_at_level(
                    pool,
                    level,
                    target,
                    "openrouter-snapshot",
                    DisplayNameReason::ResolvedOpenRouter,
                );
                if !matches!(candidate, CandidateName::None) {
                    return candidate;
                }
            }
        }
        CandidateName::None
    }

    /// 裸 ID 在 models.dev 侧的唯一身份目标（随包目录优先，其次 models.dev 快照）。
    fn modelsdev_bare_target(&self, leaf: &str) -> Result<Option<String>, DisplayNameReason> {
        if let Some(targets) = self.embedded.canonical_leaf.get(leaf) {
            if targets.len() == 1 {
                return Ok(Some(targets.iter().next().unwrap().clone()));
            }
            if targets.len() > 1 {
                return Err(DisplayNameReason::AmbiguousIdentity);
            }
        }
        if let Some(targets) = self.embedded.alias_leaf.get(leaf) {
            if targets.len() == 1 {
                return Ok(Some(targets.iter().next().unwrap().clone()));
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
            return Ok(Some(claimed.iter().next().unwrap().clone()));
        }
        if claimed.len() > 1 {
            return Err(DisplayNameReason::AmbiguousIdentity);
        }
        match raw.len() {
            1 => Ok(Some(raw.iter().next().unwrap().clone())),
            0 => Ok(None),
            _ => Err(DisplayNameReason::AmbiguousIdentity),
        }
    }

    /// 裸 ID 在 OpenRouter 侧的唯一身份目标；忽略 `~` 动态路由指针（仅允许完整 ID 命中）。
    fn openrouter_bare_target(&self, leaf: &str) -> OrBareMatch {
        if self.openrouter_availability != SnapshotAvailability::Available {
            return OrBareMatch::Unavailable(self.openrouter_availability);
        }
        let mut targets: BTreeSet<String> = BTreeSet::new();
        for key in &self.openrouter_ids {
            if key.starts_with('~') {
                continue;
            }
            if ModelIdentity::parse(key).leaf_key() != leaf {
                continue;
            }
            let canon = self
                .openrouter_explicit_canonical(key)
                .unwrap_or_else(|| key.clone());
            targets.insert(canon);
        }
        match targets.len() {
            0 => OrBareMatch::NoMatch,
            1 => OrBareMatch::Unique {
                target: targets.into_iter().next().unwrap(),
            },
            _ => OrBareMatch::Ambiguous,
        }
    }

    /// 合并同一目标身份下 models.dev 与 OpenRouter 的候选名：
    /// 优先级：随包 models.json > models.dev 快照正式名 > OpenRouter 正式名 >
    /// models.dev 快照 ID 抄写 > OpenRouter ID 抄写。
    fn combine_same_target(
        &self,
        target: &str,
        md: CandidateName,
        or: CandidateName,
        unmatched_reason: DisplayNameReason,
    ) -> DisplayNameResolution {
        let or_status_if_unused = match &or {
            CandidateName::Snapshot { .. } | CandidateName::Catalog { .. } => {
                SourceLookupStatus::Matched
            }
            CandidateName::ConflictingNames => SourceLookupStatus::AmbiguousOrConflict,
            CandidateName::None => self.openrouter_availability.no_match_status(),
        };
        let md_status_if_unused = match &md {
            CandidateName::Catalog { .. } | CandidateName::Snapshot { .. } => {
                SourceLookupStatus::Matched
            }
            CandidateName::ConflictingNames => SourceLookupStatus::AmbiguousOrConflict,
            CandidateName::None => self.modelsdev_availability.no_match_status(),
        };

        // 1. models.dev 随包规范名始终最高优先。
        if let CandidateName::Catalog { name, canonical_id } = md {
            return DisplayNameResolution {
                name: Some(name),
                canonical_id: Some(canonical_id),
                source: "models.json",
                reason: DisplayNameReason::ResolvedCatalog,
                modelsdev_status: SourceLookupStatus::Matched,
                openrouter_status: or_status_if_unused,
            };
        }
        // 2. models.dev 自身名称冲突保持冲突态，不由 OpenRouter 覆盖。
        if matches!(md, CandidateName::ConflictingNames) {
            return DisplayNameResolution::unresolved(
                DisplayNameReason::ConflictingNames,
                SourceLookupStatus::AmbiguousOrConflict,
                or_status_if_unused,
            );
        }
        // 3. models.dev 快照正式名优先于 OpenRouter。
        if let CandidateName::Snapshot {
            name,
            canonical_id,
            quality: 2,
            source,
            reason,
        } = md
        {
            return DisplayNameResolution {
                name: Some(name),
                canonical_id: Some(canonical_id),
                source,
                reason,
                modelsdev_status: SourceLookupStatus::Matched,
                openrouter_status: or_status_if_unused,
            };
        }
        // 4. OpenRouter 正式名补充 models.dev 缺失或仅 ID 抄写的条目。
        if let CandidateName::Snapshot {
            name,
            canonical_id,
            quality: 2,
            source,
            reason,
        } = or
        {
            return DisplayNameResolution {
                name: Some(name),
                canonical_id: Some(canonical_id),
                source,
                reason,
                modelsdev_status: md_status_if_unused,
                openrouter_status: SourceLookupStatus::Matched,
            };
        }
        // 5. models.dev ID 抄写兜底优先于 OpenRouter ID 抄写。
        if let CandidateName::Snapshot {
            name,
            canonical_id,
            source,
            reason,
            ..
        } = md
        {
            return DisplayNameResolution {
                name: Some(name),
                canonical_id: Some(canonical_id),
                source,
                reason,
                modelsdev_status: SourceLookupStatus::Matched,
                openrouter_status: or_status_if_unused,
            };
        }
        // 6. OpenRouter 自身冲突保持冲突态。
        if matches!(or, CandidateName::ConflictingNames) {
            return DisplayNameResolution::unresolved(
                DisplayNameReason::ConflictingNames,
                md_status_if_unused,
                SourceLookupStatus::AmbiguousOrConflict,
            );
        }
        // 7. OpenRouter ID 抄写作为最后兜底。
        if let CandidateName::Snapshot {
            name,
            canonical_id,
            source,
            reason,
            ..
        } = or
        {
            return DisplayNameResolution {
                name: Some(name),
                canonical_id: Some(canonical_id),
                source,
                reason,
                modelsdev_status: md_status_if_unused,
                openrouter_status: SourceLookupStatus::Matched,
            };
        }
        // 8. 两源均未给出可用名称：区分双源均已检查 vs 至少一路不可用。
        let reason = if md_status_if_unused.is_unavailable() || or_status_if_unused.is_unavailable()
        {
            DisplayNameReason::SourceUnavailable
        } else {
            unmatched_reason
        };
        let mut missing =
            DisplayNameResolution::unresolved(reason, md_status_if_unused, or_status_if_unused);
        if !target.is_empty() && reason == DisplayNameReason::NoTrustedName {
            missing.canonical_id = Some(target.to_string());
        }
        missing
    }

    /// 仅取模型 ID 自身的随包规范名，不采用统计映射、显式别名或快照关系。
    pub fn own_catalog_name(&self, model: &str) -> Option<String> {
        let key = identity_key(model)?;
        self.embedded.canonical.get(&key).cloned()
    }

    /// 解析展示名；`confirmed_base` 是统计层已核实的基础完整 ID（离线也有效）。
    pub fn resolve(&self, model: &str, confirmed_base: Option<&str>) -> DisplayNameResolution {
        let id = ModelIdentity::parse(model);
        if id.is_empty_key() {
            return DisplayNameResolution::unresolved(
                DisplayNameReason::EmptyKey,
                self.modelsdev_availability.no_match_status(),
                self.openrouter_availability.no_match_status(),
            );
        }
        let structural = id.identity_key();

        if id.namespace().is_some() {
            return self.resolve_namespaced(&structural, confirmed_base);
        }
        self.resolve_bare(id.leaf_key(), confirmed_base)
    }

    fn resolve_namespaced(
        &self,
        structural: &str,
        confirmed_base: Option<&str>,
    ) -> DisplayNameResolution {
        // 完整 ID 自身的规范名优先于统计映射、渠道别名或快照 canonical。
        let md_target = if self.embedded.canonical.contains_key(structural) {
            Some(structural.to_string())
        } else if let Some(base) = confirmed_base.and_then(identity_key) {
            Some(base)
        } else if let Some(target) = self.embedded.aliases.get(structural) {
            Some(target.clone())
        } else if let Some(target) = self.claims.get(structural) {
            Some(target.clone())
        } else if self.snapshot_ids.contains(structural) {
            Some(structural.to_string())
        } else {
            None
        };

        let md_lookup_target = md_target.as_deref().unwrap_or(structural);
        let md_candidate = self.choose_modelsdev_name(md_lookup_target);

        // OpenRouter 仅接受完整等价 ID、`openrouter/<作者>/<模型>` 显式渠道前缀，
        // 或已确认的目标身份映射；精确条目无名时继续查已确认别名，绝不按同末段借用其他渠道。
        let direct_key = if self.openrouter_availability == SnapshotAvailability::Available {
            if self.openrouter_ids.contains(structural) {
                Some(structural)
            } else if let Some(inner) = structural.strip_prefix("openrouter/")
                && inner.contains('/')
                && self.openrouter_ids.contains(inner)
            {
                Some(inner)
            } else {
                None
            }
        } else {
            None
        };
        let or_target = if let Some(target) = md_target.as_deref() {
            target.to_string()
        } else if let Some(dk) = direct_key {
            self.openrouter_explicit_canonical(dk)
                .unwrap_or_else(|| dk.to_string())
        } else if let Some(inner) = structural.strip_prefix("openrouter/")
            && inner.contains('/')
        {
            self.openrouter_explicit_canonical(inner)
                .unwrap_or_else(|| inner.to_string())
        } else {
            self.openrouter_explicit_canonical(structural)
                .unwrap_or_else(|| structural.to_string())
        };
        let or_candidate = self.choose_openrouter_name_for_target(direct_key, &or_target);
        self.combine_same_target(
            &or_target,
            md_candidate,
            or_candidate,
            DisplayNameReason::NoTrustedName,
        )
    }

    fn resolve_bare(&self, leaf: &str, confirmed_base: Option<&str>) -> DisplayNameResolution {
        let md_bare = if let Some(base) = confirmed_base.and_then(identity_key) {
            Ok(Some(base))
        } else {
            self.modelsdev_bare_target(leaf)
        };
        let or_bare = self.openrouter_bare_target(leaf);

        if let Err(reason) = md_bare {
            let or_status = match &or_bare {
                OrBareMatch::Unavailable(avail) => avail.no_match_status(),
                OrBareMatch::NoMatch => SourceLookupStatus::CheckedNoMatch,
                OrBareMatch::Ambiguous => SourceLookupStatus::AmbiguousOrConflict,
                OrBareMatch::Unique { target } => {
                    match self.choose_openrouter_name_for_target(None, target) {
                        CandidateName::Snapshot { .. } | CandidateName::Catalog { .. } => {
                            SourceLookupStatus::Matched
                        }
                        CandidateName::ConflictingNames => SourceLookupStatus::AmbiguousOrConflict,
                        CandidateName::None => SourceLookupStatus::CheckedNoMatch,
                    }
                }
            };
            return DisplayNameResolution::unresolved(
                reason,
                SourceLookupStatus::AmbiguousOrConflict,
                or_status,
            );
        }
        let md_target = md_bare.unwrap();

        if matches!(or_bare, OrBareMatch::Ambiguous) {
            let md_status = match md_target.as_deref() {
                Some(t) => match self.choose_modelsdev_name(t) {
                    CandidateName::Catalog { .. } | CandidateName::Snapshot { .. } => {
                        SourceLookupStatus::Matched
                    }
                    CandidateName::ConflictingNames => SourceLookupStatus::AmbiguousOrConflict,
                    CandidateName::None => self.modelsdev_availability.no_match_status(),
                },
                None => self.modelsdev_availability.no_match_status(),
            };
            return DisplayNameResolution::unresolved(
                DisplayNameReason::AmbiguousIdentity,
                md_status,
                SourceLookupStatus::AmbiguousOrConflict,
            );
        }

        match (md_target, or_bare) {
            (Some(t_md), OrBareMatch::Unique { target: t_or }) => {
                if t_md != t_or {
                    // 真实跨源冲突：models.dev 与 OpenRouter 将同一裸 ID 指向不同型号，
                    // 且不存在显式别名确认它们为同一模型。保留原始 ID 并报告冲突原因。
                    return DisplayNameResolution::unresolved(
                        DisplayNameReason::ConflictingSources,
                        SourceLookupStatus::AmbiguousOrConflict,
                        SourceLookupStatus::AmbiguousOrConflict,
                    );
                }
                let md_candidate = self.choose_modelsdev_name(&t_md);
                let or_candidate = self.choose_openrouter_name_for_target(None, &t_md);
                self.combine_same_target(
                    &t_md,
                    md_candidate,
                    or_candidate,
                    DisplayNameReason::NoTrustedName,
                )
            }
            (Some(t_md), OrBareMatch::NoMatch | OrBareMatch::Unavailable(_)) => {
                let md_candidate = self.choose_modelsdev_name(&t_md);
                let or_candidate = self.choose_openrouter_name_for_target(None, &t_md);
                self.combine_same_target(
                    &t_md,
                    md_candidate,
                    or_candidate,
                    DisplayNameReason::NoTrustedName,
                )
            }
            (None, OrBareMatch::Unique { target: t_or }) => {
                let md_candidate = self.choose_modelsdev_name(&t_or);
                let or_candidate = self.choose_openrouter_name_for_target(None, &t_or);
                self.combine_same_target(
                    &t_or,
                    md_candidate,
                    or_candidate,
                    DisplayNameReason::NoTrustedName,
                )
            }
            (None, OrBareMatch::NoMatch | OrBareMatch::Unavailable(_)) => {
                let md_status = self.modelsdev_availability.no_match_status();
                let or_status = self.openrouter_availability.no_match_status();
                let reason = if md_status.is_unavailable() || or_status.is_unavailable() {
                    DisplayNameReason::SourceUnavailable
                } else {
                    DisplayNameReason::NoIdentityEvidence
                };
                DisplayNameResolution::unresolved(reason, md_status, or_status)
            }
            (_, OrBareMatch::Ambiguous) => unreachable!("上方已处理 OpenRouter 歧义分支"),
        }
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
        for id_set in [&self.snapshot_ids, &self.openrouter_ids] {
            n.add(capacity_bytes(id_set.len(), std::mem::size_of::<String>())?)?;
            for key in id_set {
                n.add(string_bytes(key)?)?;
            }
        }
        for name_map in [&self.snapshot_names, &self.openrouter_names] {
            n.add(capacity_bytes(
                name_map.capacity(),
                std::mem::size_of::<(String, Vec<SnapshotName>)>() + MAP_BUCKET_OVERHEAD,
            )?)?;
            for (k, names) in name_map {
                n.add(string_bytes(k)?)?;
                n.add(crate::report::retained::vec_bytes(names)?)?;
                for name in names {
                    n.add(string_bytes(&name.display)?)?;
                    n.add(string_bytes(&name.name)?)?;
                }
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
            modelsdev_availability: SnapshotAvailability::NotSynced,
            openrouter_availability: SnapshotAvailability::NotSynced,
            snapshot_ids: BTreeSet::new(),
            claims: HashMap::new(),
            snapshot_names: HashMap::new(),
            openrouter_ids: BTreeSet::new(),
            openrouter_names: HashMap::new(),
        }
    }

    fn or_snapshot(entries: &[(&str, Option<&str>)]) -> crate::openrouter::Snapshot {
        crate::openrouter::Snapshot {
            v: 2,
            synced_at: "2026-11-26T00:00:00Z".into(),
            entries: entries
                .iter()
                .map(|(id, name)| crate::openrouter::SnapshotEntry {
                    id: (*id).to_string(),
                    name: name.map(str::to_string),
                    prompt: None,
                    completion: None,
                    cache_read: None,
                    cache_write: None,
                    overrides: Vec::new(),
                })
                .collect(),
            names: Vec::new(),
        }
    }

    fn md_snapshot(entries: &[(&str, Option<&str>, Option<&str>)]) -> crate::modelsdev::Snapshot {
        crate::modelsdev::Snapshot {
            v: 4,
            synced_at: "2026-11-26T00:00:00Z".into(),
            entries: entries
                .iter()
                .map(|(id, name, canonical)| crate::modelsdev::SnapshotEntry {
                    id: (*id).to_string(),
                    name: name.map(str::to_string),
                    canonical_model_id: canonical.map(str::to_string),
                    input: Some(1.0),
                    output: Some(2.0),
                    cache_read: None,
                    cache_write: None,
                    cache_write_5m: None,
                    cache_write_1h: None,
                    segments: Vec::new(),
                })
                .collect(),
            names: Vec::new(),
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
    fn identity_key_matches_generator_equivalence_cases() {
        assert_eq!(
            identity_key("Relay_One/Model.V1").as_deref(),
            Some("relayone/modelv1")
        );
        assert_eq!(
            identity_key("relay-one/model-v1").as_deref(),
            Some("relayone/modelv1")
        );
        assert_eq!(
            identity_key("Relay/Opus55:FREE").as_deref(),
            Some("relay/opus55:free")
        );
        assert_ne!(
            identity_key("Relay/Opus55:FREE"),
            identity_key("Relay/Opus55:thinking")
        );
        assert_eq!(
            identity_key("Relay/模型-Δ.1").as_deref(),
            Some("relay/模型Δ1")
        );
        assert_eq!(identity_key("provider/-._:free"), None);
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
            assert_eq!(r.modelsdev_status, SourceLookupStatus::Matched, "{id}");
            assert_eq!(
                r.openrouter_status,
                SourceLookupStatus::UnavailableNotSynced,
                "{id}"
            );
        }
        // 离线时双源快照均未同步，不得将未命中写成双源确认无名。
        let offline_miss = catalog.resolve("gpt-6-astra-cc-format", None);
        assert_eq!(offline_miss.name, None);
        assert_eq!(offline_miss.reason, DisplayNameReason::SourceUnavailable);
        assert_eq!(
            offline_miss.modelsdev_status,
            SourceLookupStatus::UnavailableNotSynced
        );
        assert_eq!(
            offline_miss.openrouter_status,
            SourceLookupStatus::UnavailableNotSynced
        );

        // 双源快照均已加载且都未命中时，才标记为 NoIdentityEvidence。
        let empty_md = md_snapshot(&[]);
        let empty_or = or_snapshot(&[]);
        let both = NameCatalog::from_snapshots(
            Some(&empty_md),
            SnapshotAvailability::Available,
            Some(&empty_or),
            SnapshotAvailability::Available,
        );
        let both_miss = both.resolve("gpt-6-astra-cc-format", None);
        assert_eq!(both_miss.name, None);
        assert_eq!(both_miss.reason, DisplayNameReason::NoIdentityEvidence);
        assert_eq!(
            both_miss.modelsdev_status,
            SourceLookupStatus::CheckedNoMatch
        );
        assert_eq!(
            both_miss.openrouter_status,
            SourceLookupStatus::CheckedNoMatch
        );
    }

    #[test]
    fn dual_source_priority_fallback_conflict_and_boundaries() {
        let md = md_snapshot(&[
            ("vendor/both-same", Some("Both Same Official"), None),
            ("vendor/md-only", Some("MD Only Name"), None),
            ("vendor/md-unnamed", None, None),
            ("author-a/conflict-m", Some("Author A Conflict"), None),
            ("zhipuai/glm-alias-test", None, None),
            (
                "openrouter/z-ai/glm-alias-test",
                None,
                Some("zhipuai/glm-alias-test"),
            ),
        ]);
        let or = or_snapshot(&[
            ("vendor/both-same", Some("Vendor: Both Same OR")),
            ("vendor/or-only", Some("Vendor: OR Only")),
            ("vendor/md-unnamed", Some("Vendor: OR Filled Name")),
            ("author-b/conflict-m", Some("Author B Conflict")),
            ("z-ai/glm-alias-test", Some("Z.ai: GLM Alias Test")),
            ("vendor/or-free:free", Some("Vendor: OR Free Variant")),
            ("~vendor/md-only", Some("Vendor: Router Pointer")),
        ]);
        let catalog = NameCatalog::from_snapshots(
            Some(&md),
            SnapshotAvailability::Available,
            Some(&or),
            SnapshotAvailability::Available,
        )
        .with_embedded_json_for_tests(r#"{"models":[],"aliases":[]}"#);

        // 两源一致：models.dev 优先。
        let same = catalog.resolve("both-same", None);
        assert_eq!(same.name.as_deref(), Some("Both Same Official"));
        assert_eq!(same.reason, DisplayNameReason::ResolvedSnapshot);
        assert_eq!(same.modelsdev_status, SourceLookupStatus::Matched);
        assert_eq!(same.openrouter_status, SourceLookupStatus::Matched);

        // 仅 models.dev 有名（~ 指针不干扰裸 ID）。
        let md_only = catalog.resolve("md-only", None);
        assert_eq!(md_only.name.as_deref(), Some("MD Only Name"));
        assert_eq!(md_only.reason, DisplayNameReason::ResolvedSnapshot);
        assert_eq!(
            md_only.openrouter_status,
            SourceLookupStatus::CheckedNoMatch
        );
        // 显式查询 ~ 完整 ID 时仍可命中 OpenRouter。
        assert_eq!(
            catalog.resolve("~vendor/md-only", None).name.as_deref(),
            Some("Vendor: Router Pointer")
        );

        // 仅 OpenRouter 有名（含裸 ID、完整 ID 与 openrouter/ 前缀）。
        for q in ["or-only", "vendor/or-only", "openrouter/vendor/or-only"] {
            let r = catalog.resolve(q, None);
            assert_eq!(r.name.as_deref(), Some("Vendor: OR Only"), "{q}");
            assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{q}");
            assert_eq!(
                r.modelsdev_status,
                SourceLookupStatus::CheckedNoMatch,
                "{q}"
            );
            assert_eq!(r.openrouter_status, SourceLookupStatus::Matched, "{q}");
        }

        // models.dev 有价格但无名称 -> OpenRouter 补名（同 ID 与显式别名两种路径）。
        for q in ["md-unnamed", "vendor/md-unnamed"] {
            let r = catalog.resolve(q, None);
            assert_eq!(r.name.as_deref(), Some("Vendor: OR Filled Name"), "{q}");
            assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{q}");
        }
        for q in [
            "glm-alias-test",
            "zhipuai/glm-alias-test",
            "z-ai/glm-alias-test",
        ] {
            let r = catalog.resolve(q, None);
            assert_eq!(r.name.as_deref(), Some("Z.ai: GLM Alias Test"), "{q}");
            assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{q}");
        }

        // 两源冲突（裸 ID 指向不同发布方且无别名）：保留原始 ID 与冲突原因；完整 ID 各用各名。
        let conflict = catalog.resolve("conflict-m", None);
        assert_eq!(conflict.name, None);
        assert_eq!(conflict.reason, DisplayNameReason::ConflictingSources);
        assert_eq!(
            conflict.modelsdev_status,
            SourceLookupStatus::AmbiguousOrConflict
        );
        assert_eq!(
            conflict.openrouter_status,
            SourceLookupStatus::AmbiguousOrConflict
        );
        assert_eq!(
            catalog.resolve("author-a/conflict-m", None).name.as_deref(),
            Some("Author A Conflict")
        );
        assert_eq!(
            catalog.resolve("author-b/conflict-m", None).name.as_deref(),
            Some("Author B Conflict")
        );

        // 保留渠道、日期、Preview、:free 边界，不得借名。
        for q in [
            "other-channel/or-only",
            "vendor/or-only-preview",
            "vendor/or-only-20261126",
            "vendor/or-only:free",
            "or-only-preview",
            "or-only-20261126",
            "or-only:free",
            "vendor/or-free",
            "or-free",
        ] {
            let r = catalog.resolve(q, None);
            assert_eq!(r.name, None, "边界查询不得借名: {q} -> {r:?}");
        }
        assert_eq!(
            catalog.resolve("vendor/or-free:free", None).name.as_deref(),
            Some("Vendor: OR Free Variant")
        );
        assert_eq!(
            catalog.resolve("or-free:free", None).name.as_deref(),
            Some("Vendor: OR Free Variant")
        );
    }

    #[test]
    fn unavailable_source_is_distinguished_from_both_checked_no_match() {
        let md = md_snapshot(&[("vendor/known-md", Some("Known MD"), None)]);
        let or = or_snapshot(&[("vendor/known-or", Some("Known OR"))]);

        // OpenRouter 加载失败：models.dev 已查无匹配，OpenRouter 标记加载失败。
        let or_failed = NameCatalog::from_snapshots(
            Some(&md),
            SnapshotAvailability::Available,
            None,
            SnapshotAvailability::LoadFailed,
        )
        .with_embedded_json_for_tests(r#"{"models":[],"aliases":[]}"#);
        let r = or_failed.resolve("unknown-model", None);
        assert_eq!(r.name, None);
        assert_eq!(r.reason, DisplayNameReason::SourceUnavailable);
        assert_eq!(r.modelsdev_status, SourceLookupStatus::CheckedNoMatch);
        assert_eq!(
            r.openrouter_status,
            SourceLookupStatus::UnavailableLoadFailed
        );

        // models.dev 未同步、OpenRouter 已加载：仅 OpenRouter 有名可命中，未命中记为来源不可用。
        let md_missing = NameCatalog::from_snapshots(
            None,
            SnapshotAvailability::NotSynced,
            Some(&or),
            SnapshotAvailability::Available,
        )
        .with_embedded_json_for_tests(r#"{"models":[],"aliases":[]}"#);
        let hit = md_missing.resolve("known-or", None);
        assert_eq!(hit.name.as_deref(), Some("Known OR"));
        assert_eq!(hit.reason, DisplayNameReason::ResolvedOpenRouter);
        assert_eq!(
            hit.modelsdev_status,
            SourceLookupStatus::UnavailableNotSynced
        );
        assert_eq!(hit.openrouter_status, SourceLookupStatus::Matched);

        let miss = md_missing.resolve("unknown-model", None);
        assert_eq!(miss.name, None);
        assert_eq!(miss.reason, DisplayNameReason::SourceUnavailable);
        assert_eq!(
            miss.modelsdev_status,
            SourceLookupStatus::UnavailableNotSynced
        );
        assert_eq!(miss.openrouter_status, SourceLookupStatus::CheckedNoMatch);
    }

    #[test]
    fn openrouter_unnamed_exact_entry_falls_back_to_confirmed_alias() {
        // 复现：OpenRouter 精确条目无名时，不得遮蔽已确认指向同一目标的别名名称。
        let catalog_json = r#"{
            "models": [],
            "aliases": [
                {"id": "openrouter/alt-org/recheck-alias", "canonical_id": "publisher/recheck-model"},
                {"id": "alt-vendor/recheck-same-leaf", "canonical_id": "publisher/recheck-same-leaf"},
                {"id": "openrouter/conflict-a/recheck-bad", "canonical_id": "publisher/recheck-conflict"},
                {"id": "openrouter/conflict-b/recheck-bad", "canonical_id": "publisher/recheck-conflict"}
            ]
        }"#;
        let md = md_snapshot(&[
            ("publisher/recheck-model", None, None),
            (
                "openrouter/live-alt/recheck-live",
                None,
                Some("publisher/recheck-live"),
            ),
        ]);
        let or_pairs = [
            // 精确条目无名 + 已确认异末段别名有名
            ("publisher/recheck-model", None),
            ("alt-org/recheck-alias", Some("Recheck Good Name")),
            // 精确条目无名 + 实时 models.dev 快照确认的别名有名（且目标不在 models.dev）
            ("publisher/recheck-live", None),
            ("live-alt/recheck-live", Some("Recheck Live Name")),
            // 精确条目仅 ID 抄写 + 已确认同末段别名有正式名
            (
                "publisher/recheck-same-leaf",
                Some("publisher/recheck-same-leaf"),
            ),
            ("alt-vendor/recheck-same-leaf", Some("Recheck Same Leaf")),
            // 精确条目有正式名时仍优先于别名，不与别名误报冲突
            ("publisher/recheck-exact-wins", Some("Exact Formal Name")),
            // 精确条目无名 + 两条已确认别名名称冲突 -> 报告冲突
            ("publisher/recheck-conflict", None),
            ("conflict-a/recheck-bad", Some("Alias Name A")),
            ("conflict-b/recheck-bad", Some("Alias Name B")),
            // 精确条目无名 + 未确认的同末段其它渠道 -> 完整 ID 不得借名
            ("publisher/recheck-unconfirmed", None),
            ("stranger/recheck-unconfirmed", Some("Stranger Name")),
        ];
        for reversed in [false, true] {
            let mut entries = or_pairs.to_vec();
            if reversed {
                entries.reverse();
            }
            let or = or_snapshot(&entries);
            let catalog = NameCatalog::from_snapshots(
                Some(&md),
                SnapshotAvailability::Available,
                Some(&or),
                SnapshotAvailability::Available,
            )
            .with_embedded_json_for_tests(catalog_json);

            for q in [
                "publisher/recheck-model",
                "openrouter/publisher/recheck-model",
                "recheck-model",
            ] {
                let r = catalog.resolve(q, None);
                assert_eq!(
                    r.name.as_deref(),
                    Some("Recheck Good Name"),
                    "reversed={reversed} q={q} -> {r:?}"
                );
                assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{q}");
                assert_eq!(r.openrouter_status, SourceLookupStatus::Matched, "{q}");
            }

            for q in ["publisher/recheck-live", "recheck-live"] {
                let r = catalog.resolve(q, None);
                assert_eq!(
                    r.name.as_deref(),
                    Some("Recheck Live Name"),
                    "reversed={reversed} q={q} -> {r:?}"
                );
                assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{q}");
            }

            for q in ["publisher/recheck-same-leaf", "recheck-same-leaf"] {
                let r = catalog.resolve(q, None);
                assert_eq!(
                    r.name.as_deref(),
                    Some("Recheck Same Leaf"),
                    "reversed={reversed} q={q} -> {r:?}"
                );
                assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{q}");
            }

            for q in ["publisher/recheck-conflict", "recheck-conflict"] {
                let r = catalog.resolve(q, None);
                assert_eq!(r.name, None, "冲突别名须保留原始 ID: {q} -> {r:?}");
                assert_eq!(r.reason, DisplayNameReason::ConflictingNames, "{q}");
                assert_eq!(
                    r.openrouter_status,
                    SourceLookupStatus::AmbiguousOrConflict,
                    "{q}"
                );
            }

            let unconfirmed = catalog.resolve("publisher/recheck-unconfirmed", None);
            assert_eq!(
                unconfirmed.name, None,
                "未确认同末段渠道不得被借名: {unconfirmed:?}"
            );
            assert_eq!(
                unconfirmed.reason,
                DisplayNameReason::NoTrustedName,
                "{unconfirmed:?}"
            );
        }

        // 精确条目自带正式名时优先于已确认别名（不报冲突）。
        let exact_wins_catalog = NameCatalog::from_snapshots(
            Some(&md),
            SnapshotAvailability::Available,
            Some(&or_snapshot(&[
                ("publisher/recheck-model", Some("Exact Publisher Name")),
                ("alt-org/recheck-alias", Some("Recheck Good Name")),
            ])),
            SnapshotAvailability::Available,
        )
        .with_embedded_json_for_tests(catalog_json);
        for q in ["publisher/recheck-model", "recheck-model"] {
            let r = exact_wins_catalog.resolve(q, None);
            assert_eq!(r.name.as_deref(), Some("Exact Publisher Name"), "{q}");
            assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{q}");
        }
    }
}
