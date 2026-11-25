//! 模型统计展示身份，与报价选择和 CCS 覆盖证据无关。

use std::collections::HashMap;

use crate::model_identity::ModelIdentity;
use crate::report::retained::{ByteCount, MAP_BUCKET_OVERHEAD, capacity_bytes, string_bytes};

// 完整结构键 → 已核实的同版本基础键；离线时也有效。Fable 与原覆盖规则共享
// 既有明确映射，但其余条目都只在统计侧使用。证据见实施计划 §2。
const STATS_ALIASES: &[(&str, &str)] = &[
    ("hy3", "tencent/hy3"),
    ("tencent/hy3:free", "tencent/hy3"),
    ("claudeopus47", "anthropic/claudeopus47"),
];

fn confirmed_key(key: &str) -> Option<&'static str> {
    STATS_ALIASES
        .iter()
        .chain(super::CONFIRMED_MODEL_ALIASES)
        .find_map(|&(alias, base)| (key == alias || key == base).then_some(base))
}

#[derive(Debug, Clone, Default)]
pub(super) struct ModelStatsGrouping {
    aliases: HashMap<String, String>,
}

impl ModelStatsGrouping {
    /// models.dev 的 `canonical_model_id` 来自目录显式声明的 `base_model`。
    /// 只消费价格加载器已经校验、执行旧格式兼容处理的快照，不另行宽松解析
    /// 原文。没有有效目录时，仅使用已核实的固定别名。
    pub(super) fn from_snapshot(catalog: &crate::modelsdev::Snapshot) -> Self {
        let mut claims: HashMap<String, Option<String>> = HashMap::new();
        for entry in &catalog.entries {
            let Some(base) = entry.canonical_model_id.as_deref() else {
                continue;
            };
            let (Some((provider, route)), Some(base_key)) =
                (entry.id.split_once('/'), catalog_base_key(&entry.id, base))
            else {
                continue;
            };
            let mut record = |id: &str| {
                let key = super::normalize_model_id(id);
                if confirmed_key(&key).is_some() {
                    return;
                }
                let claim = claims.entry(key).or_insert_with(|| Some(base_key.clone()));
                if claim.as_deref() != Some(&base_key) {
                    *claim = None;
                }
            };
            if provider.is_empty() {
                continue;
            }
            record(&entry.id);
            // models.dev 的 `openrouter/<作者>/<模型>` 条目所描述的 OpenRouter
            // 路由 ID 是 `<作者>/<模型>`。不对其它渠道或裸末段做此投影。
            if provider == "openrouter" && route.contains('/') {
                record(route);
            }
        }
        let aliases = claims
            .iter()
            .filter_map(|(alias, target)| {
                let target = target.as_ref()?;
                // 基础键必须可直接用于下钻。遇到冲突、链或环时不追猜映射。
                (alias != target
                    && claims
                        .get(target)
                        .is_none_or(|claim| claim.as_ref() == Some(target)))
                .then(|| (alias.clone(), target.clone()))
            })
            .collect();
        Self { aliases }
    }

    pub(super) fn key_for(&self, model: &str) -> String {
        let key = super::normalize_model_id(model);
        confirmed_key(&key)
            .or_else(|| self.aliases.get(&key).map(String::as_str))
            .map_or(key, str::to_owned)
    }

    /// 展示名专用（v0.9.8 补修二）：只返回**确有确认依据**的基础完整 ID——
    /// 固定别名（含基础键自身）或目录显式 `base_model` 关系；没有依据时返回
    /// None，不回退输入本身。调用方据此区分「已确认基础模型」与「仅自身结构
    /// 身份」，避免把同末段的其它命名空间误当同一模型。
    pub(super) fn confirmed_base_for(&self, model: &str) -> Option<String> {
        let key = super::normalize_model_id(model);
        confirmed_key(&key)
            .or_else(|| self.aliases.get(&key).map(String::as_str))
            .map(str::to_owned)
    }

    pub(super) fn aliases_to(&self, key: &str) -> impl Iterator<Item = &str> {
        self.aliases
            .iter()
            .filter_map(move |(alias, target)| (target == key).then_some(alias.as_str()))
    }

    pub(super) fn retained_bytes(&self) -> Result<usize, String> {
        let mut n = ByteCount::default();
        n.add(capacity_bytes(
            self.aliases.capacity(),
            std::mem::size_of::<(String, String)>() + MAP_BUCKET_OVERHEAD,
        )?)?;
        for (alias, base) in &self.aliases {
            n.add(string_bytes(alias)?)?;
            n.add(string_bytes(base)?)?;
        }
        Ok(n.get())
    }
}

fn catalog_base_key(source: &str, base: &str) -> Option<String> {
    let source = ModelIdentity::parse(source);
    let base = ModelIdentity::parse(base);
    // 目录关系才是证据；末段全等仅作版本保护，不自行剥掉日期、Preview 或
    // -thinking 等有独立语义的差异。只接受明示关系中的 `:free` 计费档。
    if base
        .namespace()
        .is_none_or(|ns| ns.is_empty() || ns.contains(['/', ':']))
        || base.is_empty_key()
        || base.variant().is_some()
        || source.base_key() != base.base_key()
        || !matches!(source.variant_key().as_deref(), None | Some("free"))
    {
        return None;
    }
    let key = base.identity_key();
    Some(confirmed_key(&key).map_or(key, str::to_owned))
}
