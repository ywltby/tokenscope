//! 模型价格表：USD / 百万 token，四类单价（input / output / cache 写 / cache 读）。
//! 键与查询统一经 `normalize_model_id` 归一化（lowercase、`.` → `-`，保留
//! `vendor/` 渠道信息），再取最后一个 `/` 之后的行为匹配键（`match_key`）。
//! 匹配分阶段（Task 2A）：先完整匹配（含 variant → 无 variant 回退），完整
//! 匹配无结果时再做有边界前缀匹配（`gpt-5` 不得命中 `gpt-50`）；同一末段
//! 键可对应多个渠道/来源候选，估算时按本次请求条件逐一计算总费用，取最高
//! 者作为保守估算（并列按来源优先级 > 完整匹配 > 前缀更长 > 原始键）。
//!
//! Task 1（2026-10-06）三层来源（编译期内置表已移除，未收录模型始终
//! unknown，无任何静态 fallback）：
//! - 外置：`~/.tokenscope/pricing.toml`（用户补充/覆盖，最高优先）；
//! - models.dev：主源（在线同步 / 本地快照离线兜底，含显示名）；
//! - OpenRouter：补充源（在线同步 / 本地快照，含显示名）。
//!
//! 未收录模型返回 None，由聚合层按 unknown 单独呈现——不得按 0 静默吞掉（M1 不变量 5）。

use std::path::{Path, PathBuf};

use anyhow::Context;

use serde::{Deserialize, Serialize};

use crate::model::TokenCounts;
use crate::openrouter;

const TIER_EXTERNAL: u8 = 0;
const TIER_MODELSDEV: u8 = 1;
const TIER_OPENROUTER: u8 = 2;

/// 归一化模型标识（键与查询共用同一函数）：lowercase、`.` → `-`。
/// 保留 `vendor/` 渠道信息（Task 2A：渠道是候选元数据，不剥前缀）；
/// 变体后缀（`:free` 等）保留参与匹配。
pub fn normalize_model_id(s: &str) -> String {
    s.trim().to_ascii_lowercase().replace('.', "-")
}

/// 匹配键：归一化后取最后一个 `/` 之后的行为准（Task 2A 候选键）。
/// `nano-gpt/qwen/qwen3.8-27b:thinking` → `qwen3.8-27b:thinking`——
/// 不同渠道的同名模型汇入同一候选组，由估算按费用裁决。
fn match_key(raw: &str) -> String {
    let norm = normalize_model_id(raw);
    match norm.rfind('/') {
        Some(i) => norm[i + 1..].to_string(),
        None => norm,
    }
}

#[derive(Debug, Clone)]
struct Entry {
    /// 归一化前缀（匹配用）。
    prefix: String,
    /// 原始写法（GUI 展示用）。
    display: String,
    name: Option<String>,
    /// Task 1：基础价 + 可选分段（flat 四价读取时包装成无分段计划）。
    pub plan: PricePlan,
    tier: u8,
}

/// 单价三态（缓存读取定价解析计划 Task 2）：
/// - `Unknown`：未知（缺价 ≠ 免费，token 进 unknown，不按 0 计费）；
/// - `Fixed(v)`：明确数值（显式 0 = 免费，与"未知"严格区分）；
/// - `SameAsInput`：显式声明沿用同一解析层（基础 / 分段 / 时间档）最终
///   生效的输入单价。当前仅允许用于 `cache_read`，其他分项在加载校验时
///   整体拒绝（不静默改语义）。
///
/// serde 线格式：`Fixed` → 数字、`SameAsInput` → `"same_as_input"`、
/// `Unknown` → null。与旧 v4 索引/计划里 `Option<f64>` 的线格式完全兼容
/// （数字 / null / 缺省语义不变），因此旧索引可直接按 v5 语义读取。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum RateSpec {
    #[default]
    Unknown,
    Fixed(f64),
    SameAsInput,
}

impl serde::Serialize for RateSpec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            RateSpec::Unknown => serializer.serialize_none(),
            RateSpec::Fixed(v) => serializer.serialize_f64(*v),
            RateSpec::SameAsInput => serializer.serialize_str("same_as_input"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for RateSpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = RateSpec;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("数字单价、\"same_as_input\" 或 null（未知）")
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(RateSpec::Fixed(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(RateSpec::Fixed(v as f64))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(RateSpec::Fixed(v as f64))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                match v {
                    "same_as_input" => Ok(RateSpec::SameAsInput),
                    other => Err(E::custom(format!("未知价格声明 {other:?}"))),
                }
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(RateSpec::Unknown)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(RateSpec::Unknown)
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                d: D,
            ) -> Result<Self::Value, D::Error> {
                RateSpec::deserialize(d)
            }
        }
        deserializer.deserialize_any(V)
    }
}

impl RateSpec {
    /// `Option<f64>` → 三态（None = Unknown；显式 0 保留为 Fixed(0)）。
    /// models.dev / OpenRouter 等数值来源的保守转换入口。
    pub fn fixed_or_unknown(v: Option<f64>) -> Self {
        match v {
            Some(x) => RateSpec::Fixed(x),
            None => RateSpec::Unknown,
        }
    }

    /// 直接解析为数值单价：仅 `Fixed`；`SameAsInput` 必须由调用方按解析
    /// 顺序（先解析 input）引用输入价，`Unknown` 保持未知——任何路径都
    /// 不允许把 Unknown 变成 0。
    pub fn resolve_direct(&self) -> Option<f64> {
        match self {
            RateSpec::Fixed(v) => Some(*v),
            _ => None,
        }
    }

    /// 显式值优先：Unknown 回退到下一层（分段 > 时间规则 > 基础价）。
    /// `SameAsInput` 算显式声明，不触发回退。
    pub fn or_explicit(self, fallback: RateSpec) -> RateSpec {
        if self == RateSpec::Unknown {
            fallback
        } else {
            self
        }
    }
}

/// 断言便利桥：`Fixed(v) == Some(v)`、`Unknown == None`、
/// `SameAsInput` 与任何 `Option<f64>` 都不等（三态互不混淆）。
/// 主要供测试断言沿用 `Some(x)` 字面量书写。
impl PartialEq<Option<f64>> for RateSpec {
    fn eq(&self, other: &Option<f64>) -> bool {
        match (self, other) {
            (RateSpec::Fixed(v), Some(o)) => v == o,
            (RateSpec::Unknown, None) => true,
            _ => false,
        }
    }
}

impl PartialEq<RateSpec> for Option<f64> {
    fn eq(&self, other: &RateSpec) -> bool {
        other == self
    }
}

/// Task 1：四类单价的规范载体。每项三态（RateSpec）：缺省 Unknown——
/// 分段/基础价缺键即未知，不静默为 0；显式 0 = 免费；cache_read 可声明
/// same_as_input（同层输入价）。
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PriceRates {
    pub input: RateSpec,
    pub output: RateSpec,
    pub cache_write: RateSpec,
    pub cache_read: RateSpec,
}

impl PriceRates {
    /// 四个数值来源（models.dev / OpenRouter / 旧索引扁平字段）的保守转换：
    /// None = Unknown，显式值 = Fixed。
    pub fn from_options(
        input: Option<f64>,
        output: Option<f64>,
        cache_write: Option<f64>,
        cache_read: Option<f64>,
    ) -> Self {
        Self {
            input: RateSpec::fixed_or_unknown(input),
            output: RateSpec::fixed_or_unknown(output),
            cache_write: RateSpec::fixed_or_unknown(cache_write),
            cache_read: RateSpec::fixed_or_unknown(cache_read),
        }
    }
}

/// 计价依据（Task 1）：当前来源统一 PromptTokens；其余为将来复用保留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PricingBasis {
    #[serde(rename = "prompt_tokens")]
    PromptTokens,
    #[serde(rename = "input_tokens")]
    InputTokens,
    #[serde(rename = "output_tokens")]
    OutputTokens,
    #[serde(rename = "total_tokens")]
    TotalTokens,
}

/// 档位应用方式（Task 1）：整笔请求切换档位；marginal 为将来扩展点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PricingApplication {
    #[serde(rename = "whole_request")]
    WholeRequest,
}

/// 有界分段：`[min_tokens, max_tokens)`，max None = 无上限。
/// prices 各项 None = 沿用基础价（显式 Some(0.0) 才是真免费）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceSegment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub min_tokens: u64,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    pub prices: PriceRates,
}

/// 通用价格计划：基础价 + 零个或多个有界分段 + 零个或多个峰谷时间规则。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PricePlan {
    #[serde(default)]
    pub basis: Option<PricingBasis>,
    #[serde(default)]
    pub application: Option<PricingApplication>,
    pub base: PriceRates,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<PriceSegment>,
    /// Task 4A：峰谷时间规则。请求时间命中规则时，其价格作为分段之后的
    /// 回退层（segment ?? schedule/period ?? base）；多条规则同时命中时
    /// 各自计价取最高，不从不同规则逐项拼价。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedules: Vec<PriceSchedule>,
}

/// 峰谷时间规则（Task 4A）：规则时区下的左闭右开时间窗 + 价格覆盖。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PriceSchedule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// IANA 时区名（如 "Asia/Shanghai"）；None = UTC。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub periods: Vec<SchedulePeriod>,
    /// 规则级价格（无 period 命中时作为该规则的基础覆盖）。
    pub prices: PriceRates,
    /// 规则内可选上下文分段（缺省时沿用 plan.segments）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<PriceSegment>,
}

/// 一段时间窗 + 价格覆盖：`[start_time, end_time)` 本地规则时区，
/// "HH:MM"；weekdays None = 每天星期限制。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SchedulePeriod {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub start_time: String,
    pub end_time: String,
    /// 星期限制（规范化小写三字母 mon/tue/wed/thu/fri/sat/sun）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekdays: Option<Vec<String>>,
    pub prices: PriceRates,
}

/// 分段区间不变量：有序、不重叠、无空洞、无上限段必须最后。
/// 返回 Err = 不可计价的规则（调用方跳过并给出诊断，不猜测）。
pub(crate) fn validate_segment_rules(segments: &[PriceSegment]) -> Result<(), String> {
    let mut prev_max: Option<u64> = None;
    for (i, s) in segments.iter().enumerate() {
        if s.max_tokens.is_some_and(|m| m <= s.min_tokens) {
            return Err(format!(
                "分段 {i}: max_tokens({:?}) 必须大于 min_tokens({})",
                s.max_tokens, s.min_tokens
            ));
        }
        match prev_max {
            Some(pm) => {
                if s.min_tokens < pm {
                    return Err(format!(
                        "分段 {i}: min_tokens({}) 与上一分段重叠（上一段上限 {pm}）",
                        s.min_tokens
                    ));
                }
                if s.min_tokens > pm {
                    return Err(format!(
                        "分段 {i}: min_tokens({}) 与上一分段上限 {pm} 存在空洞",
                        s.min_tokens
                    ));
                }
            }
            // 上一段无上限：其后不允许再有分段（非单调/覆盖不明）。
            None if i > 0 => {
                return Err(format!("分段 {i}: 上一分段无上限，其后不允许再有分段"));
            }
            None => {}
        }
        prev_max = s.max_tokens;
    }
    Ok(())
}

/// 校验价格计划：非负价格、区间有序不重叠、有上限分段后不得再有分段；
/// `SameAsInput` 只允许出现在 cache_read（其他分项整条拒绝，不猜语义）。
/// 返回 Err = 不可计价的规则（调用方跳过并给出诊断，不猜测）。
pub fn validate_price_plan(plan: &PricePlan) -> Result<(), String> {
    let mut groups: Vec<(String, &PriceRates)> = vec![("base".to_string(), &plan.base)];
    groups.extend(
        plan.segments
            .iter()
            .enumerate()
            .map(|(i, s)| (format!("分段 {i}"), &s.prices)),
    );
    for (name, r) in groups {
        if let Some((comp, v)) = negative_rate(r) {
            return Err(format!("{name} 分项 {comp} 价格为负: {v}"));
        }
        if let Some(comp) = same_as_input_violation(r) {
            return Err(format!(
                "{name} 分项 {comp} 不支持 same_as_input（当前仅 cache_read 可声明沿用输入价）"
            ));
        }
    }
    validate_segment_rules(&plan.segments)
}

/// 价格组内负值检查（Fixed 才是数值；Unknown/SameAsInput 无数值语义）。
fn negative_rate(r: &PriceRates) -> Option<(&'static str, f64)> {
    for (comp, spec) in [
        ("input", r.input),
        ("output", r.output),
        ("cache_write", r.cache_write),
        ("cache_read", r.cache_read),
    ] {
        if let RateSpec::Fixed(v) = spec
            && v < 0.0
        {
            return Some((comp, v));
        }
    }
    None
}

/// SameAsInput 白名单：仅 cache_read 可声明沿用输入价。
fn same_as_input_violation(r: &PriceRates) -> Option<&'static str> {
    for (comp, spec, allowed) in [
        ("input", r.input, false),
        ("output", r.output, false),
        ("cache_write", r.cache_write, false),
        ("cache_read", r.cache_read, true),
    ] {
        if spec == RateSpec::SameAsInput && !allowed {
            return Some(comp);
        }
    }
    None
}

/// 选择唯一满足 min <= basis < max 的分段（不做最近档位猜测）。
pub fn select_segment(plan: &PricePlan, basis_value: u64) -> Option<&PriceSegment> {
    select_segment_in(&plan.segments, basis_value)
}

/// 在给定分段集合中选择唯一命中段（Task 4A：时间规则可自带分段）。
fn select_segment_in(segments: &[PriceSegment], basis_value: u64) -> Option<&PriceSegment> {
    segments
        .iter()
        .find(|s| basis_value >= s.min_tokens && s.max_tokens.is_none_or(|m| basis_value < m))
}

/// 逐字段执行"分段显式值 > 时间规则值 > 基础值"（Task 4A 加入时间层）。
/// RateSpec 语义：Unknown 才回退下一层；Fixed(0)/SameAsInput 都是显式声明。
pub fn effective_rates(
    plan: &PricePlan,
    schedule_prices: Option<&PriceRates>,
    selected: Option<&PriceSegment>,
) -> PriceRates {
    let seg = selected.map(|s| &s.prices);
    let time = schedule_prices;
    let layer = |seg_v: RateSpec, time_v: RateSpec, base_v: RateSpec| -> RateSpec {
        seg_v.or_explicit(time_v).or_explicit(base_v)
    };
    PriceRates {
        input: layer(
            seg.map(|s| s.input).unwrap_or_default(),
            time.map(|r| r.input).unwrap_or_default(),
            plan.base.input,
        ),
        output: layer(
            seg.map(|s| s.output).unwrap_or_default(),
            time.map(|r| r.output).unwrap_or_default(),
            plan.base.output,
        ),
        cache_write: layer(
            seg.map(|s| s.cache_write).unwrap_or_default(),
            time.map(|r| r.cache_write).unwrap_or_default(),
            plan.base.cache_write,
        ),
        cache_read: layer(
            seg.map(|s| s.cache_read).unwrap_or_default(),
            time.map(|r| r.cache_read).unwrap_or_default(),
            plan.base.cache_read,
        ),
    }
}

/// 来源标识（稳定 ID，序列化用；展示层本地化在 GUI）。
fn tier_name(tier: u8) -> &'static str {
    match tier {
        TIER_EXTERNAL => "external",
        TIER_MODELSDEV => "models.dev",
        _ => "openrouter",
    }
}

/// 并列 tie-break（Task 2A）：来源优先级 > 完整匹配 > 前缀更长 > 原始键。
fn tie_rank(e: &Entry, mode: MatchMode) -> (u8, u8, usize, &str) {
    let full = u8::from(matches!(
        mode,
        MatchMode::Full | MatchMode::FullVariantFallback
    ));
    (u8::MAX - e.tier, full, e.prefix.len(), e.display.as_str())
}

/// "HH:MM" → 当日秒数；非法返回 None。
fn parse_hhmm(s: &str) -> Option<i64> {
    let (h, m) = s.trim().split_once(':')?;
    let h: i64 = h.parse().ok()?;
    let m: i64 = m.parse().ok()?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(h * 3600 + m * 60)
}

/// jiff Weekday → 规范化三字母标记（与外置 TOML/索引存储一致）。
fn weekday_tag(w: jiff::civil::Weekday) -> &'static str {
    use jiff::civil::Weekday::*;
    match w {
        Monday => "mon",
        Tuesday => "tue",
        Wednesday => "wed",
        Thursday => "thu",
        Friday => "fri",
        Saturday => "sat",
        Sunday => "sun",
    }
}

/// 外置 weekday 写法 → 规范化三字母；非法显式报错。
fn normalize_weekday(s: &str) -> Result<String, String> {
    let t = s.trim().to_ascii_lowercase();
    match t.as_str() {
        "mon" | "monday" => Ok("mon".into()),
        "tue" | "tuesday" => Ok("tue".into()),
        "wed" | "wednesday" => Ok("wed".into()),
        "thu" | "thursday" => Ok("thu".into()),
        "fri" | "friday" => Ok("fri".into()),
        "sat" | "saturday" => Ok("sat".into()),
        "sun" | "sunday" => Ok("sun".into()),
        other => Err(format!("非法 weekday: {other:?}")),
    }
}

/// 请求时间命中的时间规则变体（Task 4A）：每个命中规则返回
/// （规则, 命中的 period 或 None = 规则级默认价）。规则时区非法等
/// 在加载时已校验，此处防御性跳过。
fn matching_time_rules(
    plan: &PricePlan,
    at: jiff::Timestamp,
) -> Vec<(&PriceSchedule, Option<&SchedulePeriod>)> {
    let mut out = Vec::new();
    for sched in &plan.schedules {
        let tz = match sched.timezone.as_deref() {
            Some(name) => match jiff::tz::TimeZone::get(name) {
                Ok(tz) => tz,
                Err(_) => continue,
            },
            None => jiff::tz::TimeZone::UTC,
        };
        let zoned = at.to_zoned(tz);
        let tod =
            (zoned.hour() as i64) * 3600 + (zoned.minute() as i64) * 60 + (zoned.second() as i64);
        let wd = weekday_tag(zoned.weekday());
        let mut hit = false;
        for p in &sched.periods {
            if let Some(days) = &p.weekdays
                && !days.iter().any(|d| d == wd)
            {
                continue;
            }
            let (Some(start), Some(end)) = (parse_hhmm(&p.start_time), parse_hhmm(&p.end_time))
            else {
                continue;
            };
            if tod >= start && tod < end {
                out.push((sched, Some(p)));
                hit = true;
            }
        }
        if !hit {
            out.push((sched, None));
        }
    }
    out
}

/// 分项单价来源（缓存读取定价解析计划 Task 5）：`fixed` = 明确数值单价；
/// `same_as_input` = 沿用同一解析层的输入价（unit_price 已是解析后的实际
/// 数值，金额仍由后端统一计算）；`unknown` = 无可用单价（缺价 ≠ 免费）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateKind {
    Fixed,
    SameAsInput,
    #[default]
    Unknown,
}

/// 对单个候选项按本次请求条件计价（Task 2A：从 estimate 抽出，候选比较
/// 与最终计价共用同一函数，保证 breakdown 与总价同源）。
/// Task 4A：time_rule = 命中的（规则, 可选 period）；None = 默认档。
/// 单条规则整体参与计价，不从不同规则逐项拼价。
/// 三态解析顺序（计划 Task 2 Step 2）：先解析最终 input 单价，再解析
/// cache_read——`SameAsInput` 引用同一计划/分段/时间档解析出的输入价；
/// 输入价本身未知时 cache_read 保持未知（保守，不猜 0）。
fn estimate_entry(
    e: &Entry,
    basis_value: u64,
    t: &TokenCounts,
    time_rule: Option<(&PriceSchedule, Option<&SchedulePeriod>)>,
) -> CostEstimate {
    let plan = &e.plan;
    // 分段：时间规则自带分段优先，否则计划分段。
    // 时间层价格：period 价格逐字段回退到规则级价格（period ?? schedule）。
    let (selected, time_prices) = match time_rule {
        Some((sched, period)) => {
            let selected = if sched.segments.is_empty() {
                select_segment(plan, basis_value)
            } else {
                select_segment_in(&sched.segments, basis_value)
            };
            let time_prices = match period {
                Some(p) => PriceRates {
                    input: p.prices.input.or_explicit(sched.prices.input),
                    output: p.prices.output.or_explicit(sched.prices.output),
                    cache_write: p.prices.cache_write.or_explicit(sched.prices.cache_write),
                    cache_read: p.prices.cache_read.or_explicit(sched.prices.cache_read),
                },
                None => sched.prices,
            };
            (selected, Some(time_prices))
        }
        None => (select_segment(plan, basis_value), None),
    };
    let rates = effective_rates(plan, time_prices.as_ref(), selected);
    // Task 5：逐分项生成明细行；cost 为各行小计之和（顺序固定，确定性的
    // 浮点求和）。未知分项仅在实际产生 token 时才标记不完整——零 token
    // 的未知分项不影响完整性（不变量 5）。
    let input_price = rates.input.resolve_direct();
    // 分项单价解析：Fixed → (数值, fixed)；cache_read 的 SameAsInput →
    // (输入价, same_as_input)；其余一律未知（非 cache_read 的 SameAsInput
    // 在加载校验已被拒绝，这里防御性视为未知）。
    let resolve = |spec: RateSpec, is_cache_read: bool| -> (Option<f64>, RateKind) {
        match spec {
            RateSpec::Fixed(v) => (Some(v), RateKind::Fixed),
            RateSpec::SameAsInput if is_cache_read => match input_price {
                Some(p) => (Some(p), RateKind::SameAsInput),
                None => (None, RateKind::Unknown),
            },
            _ => (None, RateKind::Unknown),
        }
    };
    let mut cost = 0.0;
    let mut complete = true;
    let mut unknown = TokenCounts::default();
    let mut lines = Vec::with_capacity(4);
    for (kind, tokens, spec, is_cache_read, unknown_slot) in [
        (
            CostLineKind::Input,
            t.input,
            rates.input,
            false,
            &mut unknown.input,
        ),
        (
            CostLineKind::Output,
            t.output,
            rates.output,
            false,
            &mut unknown.output,
        ),
        (
            CostLineKind::CacheWrite,
            t.cache_write,
            rates.cache_write,
            false,
            &mut unknown.cache_write,
        ),
        (
            CostLineKind::CacheRead,
            t.cache_read,
            rates.cache_read,
            true,
            &mut unknown.cache_read,
        ),
    ] {
        let (price, rate_kind) = resolve(spec, is_cache_read);
        match price {
            Some(p) => {
                let subtotal = tokens as f64 * p / 1_000_000.0;
                cost += subtotal;
                lines.push(CostLine {
                    kind,
                    tokens,
                    unit_price: Some(p),
                    subtotal,
                    priced: true,
                    rate_kind,
                });
            }
            None => {
                if tokens > 0 {
                    complete = false;
                    *unknown_slot = tokens;
                }
                lines.push(CostLine {
                    kind,
                    tokens,
                    unit_price: None,
                    subtotal: 0.0,
                    priced: false,
                    rate_kind: RateKind::Unknown,
                });
            }
        }
    }
    CostEstimate {
        cost,
        unknown,
        complete,
        matched: None,
        basis_value,
        // 未声明 basis 的计划按规范口径即 prompt_tokens（Task 4 数据模型）。
        basis: Some(plan.basis.unwrap_or(PricingBasis::PromptTokens)),
        segment_label: selected.and_then(|s| s.label.clone()),
        lines,
    }
}

#[derive(Debug, Clone)]
pub struct ModelPrice {
    pub plan: PricePlan,
}

/// 候选匹配方式（Task 2A）：完整匹配优先于前缀；variant 回退必须显式标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// 完整匹配（键含 variant 时要求与查询完全一致）。
    Full,
    /// 完整匹配 + variant 回退：条目无 variant，查询 variant 被舍弃。
    FullVariantFallback,
    /// 有边界前缀匹配（条目 variant 与查询一致）。
    Prefix,
    /// 有边界前缀匹配 + variant 回退。
    PrefixVariantFallback,
}

/// 请求最终命中的候选元数据（Task 2A）：可序列化，供 breakdown/日志解释
/// "选的是谁、从哪来、怎么匹配、为何是它"。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MatchedCandidate {
    /// 原始完整模型键（来源侧写法，如 `nano-gpt/qwen/qwen3.8-27b:thinking`）。
    pub raw_key: String,
    /// 渠道：原始键第一个 `/` 之前的部分；无斜杠 = None。
    pub channel: Option<String>,
    /// 来源：external / models.dev / openrouter。
    pub source: String,
    /// 命中的归一化匹配键（末段）。
    pub matched_key: String,
    /// 匹配方式。
    pub match_mode: MatchMode,
    /// 候选总数（同一末段键参与比价的条目数）。
    pub candidate_count: usize,
    /// 选择原因：本请求条件下候选中最高费用（保守估算）。
    pub reason: String,
    /// 命中的峰谷时间档标签（Task 4A；None = 未命中/无时间规则）。
    pub schedule_label: Option<String>,
    /// 命中时间档的规则时区（Task 4A）。
    pub schedule_timezone: Option<String>,
    /// 参与裁决的请求时间（Task 4A：历史事件时间，RFC3339）。
    pub request_at: Option<String>,
}

/// 单个分项的费用明细行（Task 5）：token 数、单价与小计同源，
/// `priced = false` 表示该分项无最终单价（token 进 unknown，不按 0 计费）。
/// rate_kind 区分 fixed / same_as_input（单价已解析为实际数值）/ unknown。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CostLine {
    pub kind: CostLineKind,
    pub tokens: u64,
    /// USD / 百万 token；None = 该分项缺价。
    pub unit_price: Option<f64>,
    /// tokens × unit_price / 1e6（未计价时为 0，不计入总价）。
    pub subtotal: f64,
    pub priced: bool,
    /// 单价来源（三态语义；unknown + priced=false = 缺价，不得显示为免费）。
    #[serde(default)]
    pub rate_kind: RateKind,
}

/// 四类计价分项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostLineKind {
    Input,
    Output,
    CacheWrite,
    CacheRead,
}

/// 分项计价结果（B3）：cost 为已计价分项小计；unknown 记录无法计价分项的
/// token（缺价格 ≠ 0 价格）；complete = false 表示部分计价。
/// Task 2A：含 `Vec` 元数据后从 Copy 改为 Clone。
/// Task 5：请求级 breakdown——basis 值/依据、命中分段、四行明细与
/// 候选元数据随同一结果返回；聚合与明细复用，前端不重算。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostEstimate {
    pub cost: f64,
    pub unknown: TokenCounts,
    pub complete: bool,
    /// 最终命中候选（None = 理论上不出现：estimate 返回 None 表示未收录）。
    pub matched: Option<MatchedCandidate>,
    /// 档位选择的依据值（prompt_tokens = input + cache_write + cache_read）。
    pub basis_value: u64,
    /// 计价依据（None = 未声明，按 prompt_tokens 理解）。
    pub basis: Option<PricingBasis>,
    /// 命中分段的标签（None = 未命中分段/无分段 → 基础价）。
    pub segment_label: Option<String>,
    /// 四类分项明细（顺序固定 input/output/cache_write/cache_read）。
    pub lines: Vec<CostLine>,
}

/// GUI 悬浮对照用：该前缀在 OpenRouter 层的价格（无对应模型则 None）。
#[derive(Debug, Clone, Serialize)]
pub struct OpenRouterPrice {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
    pub name: Option<String>,
}

/// GUI 设置页展示用条目（含来源与显示名）。
/// Task 8：携带完整 PricePlan 视图——计价依据、上下文分段、峰谷时间规则；
/// 四类单价 Option 化区分"未知"与显式 0（缺失分项显示"未知"，0 显示 $0）。
#[derive(Debug, Clone, Serialize)]
pub struct PricingEntry {
    /// 原始完整模型键（展示用）。
    pub prefix: String,
    pub name: Option<String>,
    /// 渠道：原始键第一个 `/` 之前的部分；无斜杠 = None。
    pub channel: Option<String>,
    /// 四类基础单价：None = 未知，Some(0) = 免费。
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_write: Option<f64>,
    pub cache_read: Option<f64>,
    /// B3：任一分项价格未知（展示层必须可见"不完整"）。
    pub incomplete: bool,
    pub source: &'static str,
    /// 计价依据（None = 未声明，按 prompt_tokens 理解）。
    pub basis: Option<PricingBasis>,
    /// 上下文分段（规范 [min, max)）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<PriceSegment>,
    /// 峰谷时间规则。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub schedules: Vec<PriceSchedule>,
    /// 有分段或时间规则（前端据此渲染档位展开）。
    pub has_tiered_pricing: bool,
    /// 同前缀 OpenRouter 条目价格；None = OpenRouter 无对应模型。
    pub openrouter: Option<OpenRouterPrice>,
}

/// 外置 pricing.toml 的反序列化结构。
#[derive(Debug, Default, serde::Deserialize)]
struct ExternalFile {
    #[serde(default)]
    model: Vec<ExternalModel>,
    /// 语义策略（缓存读取定价解析计划 Task 3）：只改写匹配候选缺失的
    /// cache_read 声明，不生成新候选、不覆盖明确数值。
    #[serde(default)]
    model_policy: Vec<ExternalModelPolicy>,
}

/// `[[model_policy]]`：对末段匹配的候选（可选 channel/source 限定）的
/// Unknown cache_read 显式声明沿用输入价。
#[derive(Debug, Default, serde::Deserialize)]
struct ExternalModelPolicy {
    prefix: String,
    /// 渠道限定（display 第一个 `/` 之前）；省略 = 匹配所有同末段候选。
    #[serde(default)]
    channel: Option<String>,
    /// 来源限定：external / models.dev / openrouter。
    #[serde(default)]
    source: Option<String>,
    /// 当前仅支持 "same_as_input"；其他值显式告警并忽略该策略。
    #[serde(default)]
    cache_read: Option<String>,
}

/// Task 4：四类价格字段可选——区分"未填写"（None，沿用/未知）与显式 0（免费）。
#[derive(Debug, Default, serde::Deserialize)]
struct ExternalModel {
    prefix: String,
    /// 计价依据：prompt_tokens（默认）| input_tokens | output_tokens |
    /// total_tokens。当前版本只实现 prompt_tokens，其余显式拒绝（不静默
    /// 改变计价含义）。
    #[serde(default)]
    basis: Option<String>,
    /// 档位应用方式：whole_request（默认）。marginal 等留作将来扩展。
    #[serde(default)]
    application: Option<String>,
    #[serde(default)]
    input: RateSpec,
    #[serde(default)]
    output: RateSpec,
    #[serde(default)]
    cache_write: RateSpec,
    #[serde(default)]
    cache_read: RateSpec,
    #[serde(default)]
    segment: Vec<ExternalSegment>,
    /// Task 4A：峰谷时间规则。Task 4 阶段仅解析——含时间规则的条目整条
    /// 忽略并给出诊断，避免用户误以为自定义价格已生效。
    #[serde(default)]
    schedule: Vec<ExternalSchedule>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ExternalSegment {
    #[serde(default)]
    label: Option<String>,
    min_tokens: u64,
    #[serde(default)]
    max_tokens: Option<u64>,
    #[serde(default)]
    basis: Option<String>,
    #[serde(default)]
    application: Option<String>,
    #[serde(default)]
    input: RateSpec,
    #[serde(default)]
    output: RateSpec,
    #[serde(default)]
    cache_write: RateSpec,
    #[serde(default)]
    cache_read: RateSpec,
}

/// 外置峰谷时间规则（Task 4A）。
#[derive(Debug, Default, serde::Deserialize)]
struct ExternalSchedule {
    #[serde(default)]
    label: Option<String>,
    /// IANA 时区名（如 "Asia/Shanghai"）；缺省 = UTC。
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    period: Vec<ExternalPeriod>,
    /// 规则级价格覆盖（period 未给价时沿用）。
    #[serde(default)]
    input: RateSpec,
    #[serde(default)]
    output: RateSpec,
    #[serde(default)]
    cache_write: RateSpec,
    #[serde(default)]
    cache_read: RateSpec,
    /// 规则内可选上下文分段（Task 4A）。
    #[serde(default)]
    segment: Vec<ExternalSegment>,
}

/// 外置一段时间窗口 + 价格覆盖（Task 4A）。
#[derive(Debug, Default, serde::Deserialize)]
struct ExternalPeriod {
    /// "HH:MM"（本地规则时区），左闭右开 [start, end)。
    start_time: String,
    end_time: String,
    /// 可选星期限制（如 ["mon","tue"]）；缺省 = 每天。
    #[serde(default)]
    weekdays: Option<Vec<String>>,
    #[serde(default)]
    input: RateSpec,
    #[serde(default)]
    output: RateSpec,
    #[serde(default)]
    cache_write: RateSpec,
    #[serde(default)]
    cache_read: RateSpec,
}

/// 支持的计价依据/应用方式白名单（Task 4：未知值显式拒绝）。
fn parse_external_basis(s: Option<&str>) -> Result<Option<PricingBasis>, String> {
    match s {
        None | Some("prompt_tokens") => Ok(Some(PricingBasis::PromptTokens)),
        Some(other) => Err(format!(
            "不支持的 basis: {other:?}（当前支持 prompt_tokens）"
        )),
    }
}

fn parse_external_application(s: Option<&str>) -> Result<Option<PricingApplication>, String> {
    match s {
        None | Some("whole_request") => Ok(Some(PricingApplication::WholeRequest)),
        Some(other) => Err(format!(
            "不支持的 application: {other:?}（当前支持 whole_request）"
        )),
    }
}

/// 外置分段 → PriceSegment（basis/application 必须与条目一致）。
fn external_segment(
    s: &ExternalSegment,
    basis: Option<PricingBasis>,
    application: Option<PricingApplication>,
) -> Result<PriceSegment, String> {
    let seg_basis = parse_external_basis(s.basis.as_deref())?;
    if seg_basis.is_some() && seg_basis != basis {
        return Err(format!(
            "分段 {} 的 basis 与条目不一致（{:?} vs {:?}）",
            s.min_tokens, s.basis, s.basis
        ));
    }
    let seg_application = parse_external_application(s.application.as_deref())?;
    if seg_application.is_some() && seg_application != application {
        return Err(format!(
            "分段 {} 的 application 与条目不一致（{:?} vs {:?}）",
            s.min_tokens, s.application, s.application
        ));
    }
    Ok(PriceSegment {
        label: s.label.clone(),
        min_tokens: s.min_tokens,
        max_tokens: s.max_tokens,
        prices: PriceRates {
            input: s.input,
            output: s.output,
            cache_write: s.cache_write,
            cache_read: s.cache_read,
        },
    })
}

/// 外置时间规则 → PriceSchedule（Task 4A）：时区、HH:MM 窗口、星期与
/// 价格合法性在此校验；非法规则整体跳过（调用方给诊断），不猜测价格。
fn external_schedule_plan(
    s: &ExternalSchedule,
    basis: Option<PricingBasis>,
    application: Option<PricingApplication>,
) -> Result<PriceSchedule, String> {
    if let Some(name) = &s.timezone {
        jiff::tz::TimeZone::get(name).map_err(|e| format!("时间规则时区非法 {name:?}: {e}"))?;
    }
    let mut periods = Vec::with_capacity(s.period.len());
    for p in &s.period {
        let start = parse_hhmm(&p.start_time)
            .ok_or_else(|| format!("时间窗 start_time 非法: {:?}", p.start_time))?;
        let end = parse_hhmm(&p.end_time)
            .ok_or_else(|| format!("时间窗 end_time 非法: {:?}", p.end_time))?;
        if start >= end {
            return Err(format!(
                "时间窗 [{}, {}) 非法（须 start < end，同日窗口）",
                p.start_time, p.end_time
            ));
        }
        let weekdays = p
            .weekdays
            .as_ref()
            .map(|days| {
                days.iter()
                    .map(|d| normalize_weekday(d))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        periods.push(SchedulePeriod {
            label: None,
            start_time: p.start_time.clone(),
            end_time: p.end_time.clone(),
            weekdays,
            prices: PriceRates {
                input: p.input,
                output: p.output,
                cache_write: p.cache_write,
                cache_read: p.cache_read,
            },
        });
    }
    let mut segments = Vec::with_capacity(s.segment.len());
    for seg in &s.segment {
        segments.push(external_segment(seg, basis, application)?);
    }
    let sched = PriceSchedule {
        label: s.label.clone(),
        timezone: s.timezone.clone(),
        periods,
        prices: PriceRates {
            input: s.input,
            output: s.output,
            cache_write: s.cache_write,
            cache_read: s.cache_read,
        },
        segments,
    };
    // 规则内价格不得为负；SameAsInput 仅限 cache_read；规则分段须满足区间不变量。
    for r in [&sched.prices]
        .into_iter()
        .chain(sched.periods.iter().map(|p| &p.prices))
        .chain(sched.segments.iter().map(|g| &g.prices))
    {
        if let Some((comp, v)) = negative_rate(r) {
            return Err(format!("时间规则分项 {comp} 价格为负: {v}"));
        }
        if let Some(comp) = same_as_input_violation(r) {
            return Err(format!(
                "时间规则分项 {comp} 不支持 same_as_input（当前仅 cache_read 可声明沿用输入价）"
            ));
        }
    }
    validate_segment_rules(&sched.segments)?;
    Ok(sched)
}

/// 外置条目 → PricePlan（Task 4）：四类价可选、分段走统一校验，
/// 任何非法规则拒绝整条并给出诊断（不静默回退到基础价）。
/// Task 4A：时间规则解析失败只跳过该规则（诊断随 Vec 返回），条目其余
/// 部分仍生效。
fn external_model_plan(m: &ExternalModel) -> Result<(PricePlan, Vec<String>), String> {
    let basis = parse_external_basis(m.basis.as_deref())?;
    let application = parse_external_application(m.application.as_deref())?;
    let base = PriceRates {
        input: m.input,
        output: m.output,
        cache_write: m.cache_write,
        cache_read: m.cache_read,
    };
    let mut segments = Vec::with_capacity(m.segment.len());
    for s in &m.segment {
        segments.push(external_segment(s, basis, application)?);
    }
    let mut rule_warnings = Vec::new();
    let mut schedules = Vec::new();
    for s in &m.schedule {
        match external_schedule_plan(s, basis, application) {
            Ok(sched) => schedules.push(sched),
            Err(err) => rule_warnings.push(err),
        }
    }
    let plan = PricePlan {
        basis,
        application,
        base,
        segments,
        schedules,
    };
    validate_price_plan(&plan)?;
    Ok((plan, rule_warnings))
}

/// 外置文件不存在时「创建模板」写入的内容。
pub const PRICING_TEMPLATE: &str = r#"# TokenScope 外置价格表（USD / 百万 token）
# 优先级：本文件 > models.dev（主源）> OpenRouter（补充源）。
# 同名模型多渠道并存时，按本次请求的 prompt token 等条件逐候选计价取
# 最高费用（保守估算）。未收录模型按未知价格处理（不猜测）。
# 模型键支持 vendor 写法（匹配取最后一个 / 之后的模型名）；修改保存后
# 下一次统计即生效。
#
# 基础价格（四类字段均可选：缺省 = 沿用/未知；显式 0 = 免费）：

[[model]]
prefix = "claude-opus-5"
input = 5.0
output = 25.0
cache_write = 6.25
cache_read = 0.5

# 上下文分段（可选）：[min_tokens, max_tokens) 左闭右开，末档不写
# max_tokens 表示无上限。分段缺某类价格时沿用基础价。

# [[model.segment]]
# label = ">272K"
# min_tokens = 272001
# input = 8.0
# output = 30.0
# cache_write = 10.0
# cache_read = 0.8
#
# 语义策略（可选）：为价格快照缺 cache_read 的候选显式声明
# 「缓存读按输入价计费」。只改写缺失值，不覆盖已有数字/显式 0；
# channel 省略 = 匹配所有同末段候选；没有候选命中时给出 warning
# （不静默制造价格、不生成新候选）。
#
# [[model_policy]]
# prefix = "gpt-5.4"
# channel = "zenmux"
# cache_read = "same_as_input"
"#;

/// 拆出变体后缀：`"hy3:free"` → `("hy3", Some("free"))`；无变体 → `("hy3", None)`。
fn split_variant(normalized: &str) -> (&str, Option<&str>) {
    match normalized.find(':') {
        Some(pos) => (&normalized[..pos], Some(&normalized[pos + 1..])),
        None => (normalized, None),
    }
}

/// 前缀索引：归一化前缀（字节串，避免多字节切片 panic）→ 同前缀全部条目。
/// 查找时只枚举查询串自身的 ~30 个前缀做哈希命中，复杂度与表大小无关。
type PrefixIndex = std::collections::HashMap<Vec<u8>, Vec<Entry>>;

/// 价格索引快照：三层合并结果的持久化形态（Task 1 起无内置层）。
/// 加载它即可跳过双快照解析与合并（扁平结构，毫秒级）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingIndex {
    /// 索引格式版本：B3 起分项可空（v2）；版本不符的旧索引按失效重建。
    #[serde(default = "default_index_v1")]
    pub v: u8,
    /// D2：重建时的诊断（快照缺失/解析失败等）随索引持久化——
    /// 缓存命中也要让用户看见降级状态，不能“命中即无声”。
    #[serde(default)]
    pub warnings: Vec<String>,
    /// 三源签名（路径+大小+mtime），用于判断是否需要重建。
    pub sig: String,
    pub synced_at: String,
    pub entries: Vec<IndexEntry>,
}

fn default_index_v1() -> u8 {
    1
}

/// 索引格式当前版本。
/// Task 2A：v4——匹配键改为末段模型名（保留渠道候选元数据）。
/// v5（缓存读取定价解析计划）：单价升级为三态 RateSpec。线格式向后兼容
/// （数字/null 语义不变，v4 索引可直接按 v5 语义读取：数值 → Fixed、
/// null/缺失 → Unknown、不推断 same_as_input），因此加载接受 v4 与 v5
/// （更早的 v2/v3 匹配键语义不同，仍按版本失效重建）。
pub const INDEX_VERSION: u8 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// 归一化前缀。
    pub prefix: String,
    pub display: String,
    pub name: Option<String>,
    pub tier: u8,
    /// Task 1：分段价格计划（None = 旧索引扁平四价，读取时包装为无分段计划）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<PricePlan>,
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_write: Option<f64>,
    pub cache_read: Option<f64>,
}

pub fn save_index(path: &Path, index: &PricingIndex) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(index)?;
    crate::fsutil::atomic_write(path, json.as_bytes())
        .with_context(|| format!("写价格索引失败: {}", path.display()))?;
    Ok(())
}

pub fn load_index(path: &Path) -> anyhow::Result<Option<PricingIndex>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读价格索引失败: {}", path.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .with_context(|| format!("价格索引解析失败: {}", path.display()))
}

/// 进程内缓存：签名一致则直接复用，避免每次调用重读快照。
/// Task 2：定价可用性状态（结构化 DTO，前端据此渲染横幅，不解析文本）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingStatus {
    pub modelsdev_available: bool,
    pub modelsdev_count: usize,
    pub modelsdev_synced_at: Option<String>,
    pub openrouter_available: bool,
    pub external_count: usize,
    pub has_any_pricing: bool,
    pub needs_sync: bool,
    pub warnings: Vec<String>,
}

/// 计算定价状态：与 `Pricing::load` / 设置页共用同一路径解析与快照规则，
/// 不会出现设置页显示可用而汇总页认为不可用的分叉。
pub fn pricing_status(
    external: Option<&Path>,
    modelsdev_snapshot: Option<&Path>,
    openrouter_snapshot: Option<&Path>,
) -> PricingStatus {
    let mut warnings = Vec::new();
    let (md_available, md_count, md_synced_at) =
        match modelsdev_snapshot.map(crate::modelsdev::load_snapshot) {
            Some(Ok(Some(s))) => {
                let n = s.entries.len();
                (n > 0, n, Some(s.synced_at))
            }
            Some(Ok(None)) => (false, 0, None),
            Some(Err(e)) => {
                warnings.push(format!("models.dev 快照解析失败（主源不可用）: {e:#}"));
                (false, 0, None)
            }
            None => (false, 0, None),
        };
    let or_available = match openrouter_snapshot.map(crate::openrouter::load_snapshot) {
        Some(Ok(Some(s))) => !s.entries.is_empty(),
        Some(Ok(None)) => false,
        Some(Err(e)) => {
            warnings.push(format!("OpenRouter 快照解析失败（补充源不可用）: {e:#}"));
            false
        }
        None => false,
    };
    let (pricing, load_warnings) = Pricing::load(external, modelsdev_snapshot, openrouter_snapshot);
    for w in load_warnings {
        warnings.push(w);
    }
    let external_count = pricing.external_count();
    let has_any_pricing = md_available || or_available || external_count > 0;
    PricingStatus {
        modelsdev_available: md_available,
        modelsdev_count: md_count,
        modelsdev_synced_at: md_synced_at,
        openrouter_available: or_available,
        external_count,
        has_any_pricing,
        needs_sync: !md_available,
        warnings,
    }
}

/// 进程内价格缓存条目：签名 → (价格表, 重建时诊断)。
type PriceCacheEntry = (String, std::sync::Arc<Pricing>, Vec<String>);
static PRICE_CACHE: std::sync::Mutex<Option<PriceCacheEntry>> = std::sync::Mutex::new(None);

fn source_sig(paths: &[Option<PathBuf>]) -> String {
    let mut parts = Vec::new();
    for p in paths {
        match p {
            Some(path) => {
                let meta = std::fs::metadata(path).ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let mtime = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                parts.push(format!("{}:{}:{}", path.display(), size, mtime));
            }
            None => parts.push("-".to_string()),
        }
    }
    parts.join("|")
}

#[derive(Debug, Clone)]
pub struct Pricing {
    by_prefix: PrefixIndex,
}

impl Default for Pricing {
    /// Task 1：默认空表——没有显式来源时不会偷偷给价（不变量 1）。
    fn default() -> Self {
        Self::empty()
    }
}

impl Pricing {
    /// 空价格表（无任何来源时的起点）。
    pub fn empty() -> Self {
        Self {
            by_prefix: PrefixIndex::new(),
        }
    }

    fn add_entry(&mut self, e: Entry) {
        self.by_prefix
            .entry(e.prefix.as_bytes().to_vec())
            .or_default()
            .push(e);
    }

    fn all_entries(&self) -> impl Iterator<Item = &Entry> {
        self.by_prefix.values().flatten()
    }

    /// 导出索引快照（同步后/重建后写入，供下次启动快速加载）。
    pub fn to_index(&self, sig: String, synced_at: String, warnings: Vec<String>) -> PricingIndex {
        PricingIndex {
            v: INDEX_VERSION,
            warnings,
            sig,
            synced_at,
            entries: self
                .all_entries()
                .map(|e| IndexEntry {
                    prefix: e.prefix.clone(),
                    display: e.display.clone(),
                    name: e.name.clone(),
                    tier: e.tier,
                    plan: Some(e.plan.clone()),
                    input: e.plan.base.input.resolve_direct(),
                    output: e.plan.base.output.resolve_direct(),
                    cache_write: e.plan.base.cache_write.resolve_direct(),
                    cache_read: e.plan.base.cache_read.resolve_direct(),
                })
                .collect(),
        }
    }

    /// 从索引恢复（扁平结构，跳过双快照解析与合并）。
    pub fn from_index(index: &PricingIndex) -> Self {
        let mut pricing = Self {
            by_prefix: PrefixIndex::new(),
        };
        for e in &index.entries {
            let plan = e.plan.clone().unwrap_or_else(|| PricePlan {
                base: PriceRates::from_options(e.input, e.output, e.cache_write, e.cache_read),
                ..Default::default()
            });
            pricing.add_entry(Entry {
                prefix: e.prefix.clone(),
                display: e.display.clone(),
                name: e.name.clone(),
                plan,
                tier: e.tier,
            });
        }
        pricing
    }

    /// 三层合并（Task 1）：openrouter/models.dev 快照叠加在空表上，外置最终覆盖。
    /// 各快照文件缺失 → 静默跳过该层；解析失败 → 警告并跳过该层。
    pub fn load(
        external: Option<&Path>,
        modelsdev_snapshot: Option<&Path>,
        openrouter_snapshot: Option<&Path>,
    ) -> (Self, Vec<String>) {
        // Task 1：三层来源从空表叠加，没有任何编译期 fallback。
        let mut pricing = Self::empty();
        let mut warnings = Vec::new();

        if let Some(path) = modelsdev_snapshot {
            match crate::modelsdev::load_snapshot(path) {
                Ok(Some(snapshot)) => {
                    for e in snapshot.entries {
                        // Task 2：快照分段（models.dev size 语义已在同步时
                        // 转换为规范 [min, max)）→ PriceSegment。非法规则
                        // 防御性丢弃分段只留基础价，并给出诊断。
                        let segments = e
                            .segments
                            .iter()
                            .map(|s| PriceSegment {
                                label: s.label.clone(),
                                min_tokens: s.min_tokens,
                                max_tokens: s.max_tokens,
                                prices: PriceRates::from_options(
                                    s.input,
                                    s.output,
                                    s.cache_write,
                                    s.cache_read,
                                ),
                            })
                            .collect::<Vec<_>>();
                        let base = PriceRates::from_options(
                            e.input,
                            e.output,
                            e.cache_write,
                            e.cache_read,
                        );
                        let plan = PricePlan {
                            basis: Some(PricingBasis::PromptTokens),
                            application: Some(PricingApplication::WholeRequest),
                            base,
                            segments: segments.clone(),
                            schedules: Vec::new(),
                        };
                        let plan = match validate_price_plan(&plan) {
                            Ok(()) => plan,
                            Err(err) => {
                                warnings.push(format!(
                                    "models.dev 条目 {} 分段规则非法（{err}），仅保留基础价",
                                    e.id
                                ));
                                PricePlan {
                                    basis: Some(PricingBasis::PromptTokens),
                                    application: Some(PricingApplication::WholeRequest),
                                    base,
                                    segments: Vec::new(),
                                    schedules: Vec::new(),
                                }
                            }
                        };
                        pricing.add_entry(Entry {
                            prefix: match_key(&e.id),
                            display: e.id,
                            name: e.name,
                            plan,
                            tier: TIER_MODELSDEV,
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => warnings.push(format!(
                    "models.dev 快照解析失败，该层已忽略: {}（{e:#}）",
                    path.display()
                )),
            }
        }

        if let Some(path) = openrouter_snapshot {
            match openrouter::load_snapshot(path) {
                Ok(Some(snapshot)) => {
                    for e in snapshot.entries {
                        // Task 3：OpenRouter overrides（USD/token，inclusive
                        // 下界：prompt >= min 即命中）→ 规范分段 [min,
                        // next_min) 并 ×1e6 统一为 USD/百万 token；基础价
                        // 覆盖最低阈值以下。基础价维持 prompt/completion
                        // 两价口径（缓存分项按未知，R05 → C5）。
                        let segments = e
                            .overrides
                            .iter()
                            .enumerate()
                            .map(|(i, o)| {
                                let conv = |v: Option<f64>| {
                                    RateSpec::fixed_or_unknown(v.map(|p| p * 1_000_000.0))
                                };
                                PriceSegment {
                                    label: Some(format!("≥{}", o.min_prompt_tokens)),
                                    min_tokens: o.min_prompt_tokens,
                                    max_tokens: e.overrides.get(i + 1).map(|n| n.min_prompt_tokens),
                                    prices: PriceRates {
                                        input: conv(o.prompt),
                                        output: conv(o.completion),
                                        cache_write: conv(o.cache_write),
                                        cache_read: conv(o.cache_read),
                                    },
                                }
                            })
                            .collect::<Vec<_>>();
                        pricing.add_entry(Entry {
                            prefix: match_key(&e.id),
                            display: e.id,
                            name: e.name,
                            plan: PricePlan {
                                basis: Some(PricingBasis::PromptTokens),
                                application: Some(PricingApplication::WholeRequest),
                                base: PriceRates {
                                    input: RateSpec::fixed_or_unknown(
                                        e.prompt.map(|v| v * 1_000_000.0),
                                    ),
                                    output: RateSpec::fixed_or_unknown(
                                        e.completion.map(|v| v * 1_000_000.0),
                                    ),
                                    cache_write: RateSpec::Unknown,
                                    cache_read: RateSpec::Unknown,
                                },
                                segments,
                                schedules: Vec::new(),
                            },
                            tier: TIER_OPENROUTER,
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => warnings.push(format!(
                    "OpenRouter 快照解析失败，该层已忽略: {}（{e:#}）",
                    path.display()
                )),
            }
        }

        if let Some(path) = external {
            let Ok(text) = std::fs::read_to_string(path) else {
                return (pricing, warnings);
            };
            let parsed: ExternalFile = match toml::from_str(&text) {
                Ok(p) => p,
                Err(e) => {
                    warnings.push(format!(
                        "外置价格文件解析失败，该层已忽略: {}（{e}）",
                        path.display()
                    ));
                    return (pricing, warnings);
                }
            };
            for m in parsed.model {
                let display = m.prefix.clone();
                match external_model_plan(&m) {
                    Ok((plan, rule_warnings)) => {
                        for w in rule_warnings {
                            warnings.push(format!("外置条目 {display}: {w}"));
                        }
                        pricing.add_entry(Entry {
                            prefix: match_key(&m.prefix),
                            display,
                            name: None,
                            plan,
                            tier: TIER_EXTERNAL,
                        });
                    }
                    Err(err) => warnings.push(format!("外置条目 {display} 已忽略: {err}")),
                }
            }
            pricing.apply_model_policies(&parsed.model_policy, &mut warnings);
        }

        (pricing, warnings)
    }

    /// 应用 [[model_policy]]（Task 3）：只把匹配候选的 Unknown cache_read
    /// 改写为声明值；已有明确数字/显式 0 不覆盖；无命中给出 warning，
    /// 不静默制造价格、不生成新候选。
    fn apply_model_policies(
        &mut self,
        policies: &[ExternalModelPolicy],
        warnings: &mut Vec<String>,
    ) {
        for pol in policies {
            let spec = match pol.cache_read.as_deref() {
                Some("same_as_input") => RateSpec::SameAsInput,
                Some(other) => {
                    warnings.push(format!(
                        "外置 model_policy {:?}: 不支持的 cache_read 值 {other:?}（仅支持 \"same_as_input\"），策略已忽略",
                        pol.prefix
                    ));
                    continue;
                }
                None => {
                    warnings.push(format!(
                        "外置 model_policy {:?}: 未声明 cache_read，策略已忽略",
                        pol.prefix
                    ));
                    continue;
                }
            };
            let key = match_key(&pol.prefix);
            let mut hits = 0usize;
            let mut skipped_explicit = 0usize;
            if let Some(group) = self.by_prefix.get_mut(key.as_bytes()) {
                for e in group {
                    if let Some(ch) = &pol.channel
                        && e.display.split_once('/').map(|(c, _)| c) != Some(ch.as_str())
                    {
                        continue;
                    }
                    if let Some(src) = &pol.source
                        && tier_name(e.tier) != src.as_str()
                    {
                        continue;
                    }
                    hits += 1;
                    if e.plan.base.cache_read == RateSpec::Unknown {
                        e.plan.base.cache_read = spec;
                        log::info!("model_policy 生效：{} cache_read 沿用输入价", e.display);
                    } else {
                        skipped_explicit += 1;
                    }
                }
            }
            if hits == 0 {
                warnings.push(format!(
                    "外置 model_policy {:?}: 没有命中任何候选（模型未收录或 channel/source 限定过严），未生成价格",
                    pol.prefix
                ));
            } else if skipped_explicit > 0 {
                log::debug!(
                    "model_policy {:?}: {skipped_explicit} 个候选已有明确 cache_read，未被覆盖",
                    pol.prefix
                );
            }
        }
    }

    /// 层级优先（外置 > models.dev > OpenRouter），并列时完整匹配优先、
    /// 前缀更长优先、原始键字典序（Task 2A tie-break，不含请求条件）。
    /// **计费入口是 `estimate`**：它按本次请求条件对候选逐一并取最高费用；
    /// `lookup` 仅用于无请求上下文的快速判定（测试/预热）。
    ///
    /// 热路径（每事件一次 × 数千条目）：匹配键在加载时已归一化为末段，
    /// 这里零分配枚举查询末段自身的前缀（≤30 个）做哈希命中。
    pub fn lookup(&self, model: &str) -> Option<ModelPrice> {
        let cands = self.collect_candidates(model);
        let best =
            cands
                .into_iter()
                .fold(None::<(&Entry, MatchMode)>, |acc, (e, m)| match acc {
                    Some((be, bm)) if tie_rank(be, bm) >= tie_rank(e, m) => Some((be, bm)),
                    _ => Some((e, m)),
                })?;
        Some(ModelPrice {
            plan: best.0.plan.clone(),
        })
    }

    /// 候选收集（Task 2A，分阶段回退，阶段内不混匹配方式）：
    /// 1) 完整匹配（含 variant，键完全一致）；
    /// 2) 完整匹配 variant 回退（查询带 variant 但条目只有基名）；
    /// 3) 有边界前缀匹配（条目 variant 必须与查询一致）；
    /// 4) 有边界前缀 + variant 回退。
    ///
    /// 前缀必须停在词元边界：`qwen3.8-27b` 可命中 `qwen3.8-27b-instruct`，
    /// 不得命中 `qwen3.8-27b2`。免费（`:free`）等变体条目只在查询 variant
    /// 完全一致时命中，基名价格对变体查询的套用仅发生在显式标记的回退段。
    fn collect_candidates(&self, model: &str) -> Vec<(&Entry, MatchMode)> {
        let leaf = match_key(model);
        let (base, variant) = split_variant(&leaf);
        let mut cands: Vec<(&Entry, MatchMode)> = Vec::new();

        // 1) 完整匹配（含 variant）。
        if let Some(group) = self.by_prefix.get(leaf.as_bytes()) {
            cands.extend(group.iter().map(|e| (e, MatchMode::Full)));
        }
        if !cands.is_empty() {
            return cands;
        }
        // 2) 完整匹配 variant 回退（查询带 variant、条目只有基名）。
        if variant.is_some()
            && let Some(group) = self.by_prefix.get(base.as_bytes())
        {
            cands.extend(group.iter().map(|e| (e, MatchMode::FullVariantFallback)));
        }
        if !cands.is_empty() {
            return cands;
        }
        // 3) 有边界前缀匹配（变体一致；变体携带的条目只会精确命中）。
        let bytes = leaf.as_bytes();
        for k in (1..bytes.len()).rev() {
            if let Some(group) = self.by_prefix.get(&bytes[..k]) {
                for e in group {
                    let (_, evar) = split_variant(&e.prefix);
                    if evar != variant {
                        continue;
                    }
                    let rest = &leaf[k..];
                    if rest.starts_with('-') || rest.starts_with(':') {
                        cands.push((e, MatchMode::Prefix));
                    }
                }
            }
        }
        if !cands.is_empty() {
            return cands;
        }
        // 4) 前缀 + variant 回退（对基名做有边界前缀，条目必须无 variant）。
        if variant.is_some() {
            let bbytes = base.as_bytes();
            for k in (1..bbytes.len()).rev() {
                if let Some(group) = self.by_prefix.get(&bbytes[..k]) {
                    for e in group {
                        let (_, evar) = split_variant(&e.prefix);
                        if evar.is_some() {
                            continue;
                        }
                        let rest = &base[k..];
                        if rest.starts_with('-') || rest.starts_with(':') {
                            cands.push((e, MatchMode::PrefixVariantFallback));
                        }
                    }
                }
            }
        }
        cands
    }

    /// 返回 None 表示模型未收录（unknown），不是 0 费用。
    /// 分项计价（B3）：已知分项计价求和；未知分项（None）的 token 进
    /// unknown 且 complete=false——缺价格 ≠ 0 价格。
    /// Task 2A：先按末段键收集候选，再对每个候选用**本次请求条件**
    /// （basis token、分段规则与事件时间）计算总费用，取最高者作为保守
    /// 估算；并列按来源优先级 > 完整匹配 > 前缀更长 > 原始键打破。
    /// 禁止跨候选拼价：四类单价全部来自最终选中的同一个候选。
    /// Task 4A：`at` 是**历史事件时间**（调用方必须传事件 timestamp，
    /// 不得用当前墙上时钟替代）；候选的峰谷规则按它换算命中档，
    /// 同候选多条规则命中时各自计价取最高。
    pub fn estimate(
        &self,
        model: &str,
        t: &TokenCounts,
        at: jiff::Timestamp,
    ) -> Option<CostEstimate> {
        let cands = self.collect_candidates(model);
        if cands.is_empty() {
            return None;
        }
        // Task 1（分段计价）：basis = prompt token（input + cache_write +
        // cache_read），整笔请求切换档位；输出不参与档位选择（不变量 2）。
        let basis_value = t.prompt_tokens();
        let cand_count = cands.len();
        let mut best: Option<(CostEstimate, MatchMode, &Entry, Option<&PriceSchedule>)> = None;
        for (e, mode) in cands {
            // 候选内变体择优：默认档 + 每个命中时间规则（整规则一套价）；
            // 费用并列时保守保留默认档。
            let mut vbest = (estimate_entry(e, basis_value, t, None), None);
            for (sched, period) in matching_time_rules(&e.plan, at) {
                let est = estimate_entry(e, basis_value, t, Some((sched, period)));
                if est.cost > vbest.0.cost {
                    vbest = (est, Some(sched));
                }
            }
            let (est, sched) = vbest;
            let take = match &best {
                None => true,
                Some((b, bm, be, _)) => {
                    est.cost > b.cost
                        || (est.cost == b.cost && tie_rank(e, mode) > tie_rank(be, *bm))
                }
            };
            if take {
                best = Some((est, mode, e, sched));
            }
        }
        let (mut est, mode, e, sched) = best.unwrap();
        est.matched = Some(MatchedCandidate {
            raw_key: e.display.clone(),
            channel: e.display.split_once('/').map(|(c, _)| c.to_string()),
            source: tier_name(e.tier).to_string(),
            matched_key: e.prefix.clone(),
            match_mode: mode,
            candidate_count: cand_count,
            reason: "candidates_highest_cost".to_string(),
            schedule_label: sched.and_then(|s| s.label.clone()),
            schedule_timezone: sched.and_then(|s| s.timezone.clone()),
            request_at: Some(at.to_string()),
        });
        Some(est)
    }

    /// GUI 设置页条目（合并视图，含来源、显示名与同前缀 OpenRouter 对照价）。
    pub fn entries(&self) -> Vec<PricingEntry> {
        // openrouter 层按归一化前缀索引，供非 openrouter 行对照（精确同前缀）。
        let or_by_prefix: std::collections::HashMap<&str, &Entry> = self
            .all_entries()
            .filter(|e| e.tier == TIER_OPENROUTER)
            .map(|e| (e.prefix.as_str(), e))
            .collect();
        self.all_entries()
            .map(|e| {
                let openrouter = or_by_prefix
                    .get(e.prefix.as_str())
                    .map(|o| OpenRouterPrice {
                        input: o.plan.base.input.resolve_direct().unwrap_or(0.0),
                        output: o.plan.base.output.resolve_direct().unwrap_or(0.0),
                        cache_write: o.plan.base.cache_write.resolve_direct().unwrap_or(0.0),
                        cache_read: o.plan.base.cache_read.resolve_direct().unwrap_or(0.0),
                        name: o.name.clone(),
                    });
                PricingEntry {
                    prefix: e.display.clone(),
                    name: e.name.clone(),
                    channel: e.display.split_once('/').map(|(c, _)| c.to_string()),
                    input: e.plan.base.input.resolve_direct(),
                    output: e.plan.base.output.resolve_direct(),
                    cache_write: e.plan.base.cache_write.resolve_direct(),
                    cache_read: e.plan.base.cache_read.resolve_direct(),
                    incomplete: e.plan.base.input == RateSpec::Unknown
                        || e.plan.base.output == RateSpec::Unknown
                        || e.plan.base.cache_write == RateSpec::Unknown
                        || e.plan.base.cache_read == RateSpec::Unknown,
                    source: match e.tier {
                        TIER_EXTERNAL => "外置",
                        TIER_MODELSDEV => "models.dev",
                        _ => "OpenRouter",
                    },
                    basis: e.plan.basis,
                    segments: e.plan.segments.clone(),
                    schedules: e.plan.schedules.clone(),
                    has_tiered_pricing: !e.plan.segments.is_empty() || !e.plan.schedules.is_empty(),
                    openrouter,
                }
            })
            .collect()
    }

    pub fn external_count(&self) -> usize {
        self.count_tier(TIER_EXTERNAL)
    }

    pub fn openrouter_count(&self) -> usize {
        self.count_tier(TIER_OPENROUTER)
    }

    pub fn modelsdev_count(&self) -> usize {
        self.count_tier(TIER_MODELSDEV)
    }

    fn count_tier(&self, tier: u8) -> usize {
        self.all_entries().filter(|e| e.tier == tier).count()
    }

    /// 带进程内缓存的加载（M11）：三源签名一致直接复用，否则重建并写索引文件。
    /// 返回 (Pricing, 警告, 是否命中缓存)。
    pub fn load_cached(
        external: Option<&Path>,
        modelsdev_snapshot: Option<&Path>,
        openrouter_snapshot: Option<&Path>,
        index_path: &Path,
    ) -> (std::sync::Arc<Pricing>, Vec<String>, bool) {
        let paths = [
            external.map(Path::to_path_buf),
            modelsdev_snapshot.map(Path::to_path_buf),
            openrouter_snapshot.map(Path::to_path_buf),
        ];
        let sig = source_sig(&paths);
        let mut guard = PRICE_CACHE.lock().unwrap();
        // D2：命中也返回持久化的诊断——降级状态必须随结果持续可见。
        if let Some((cached_sig, cached, cached_warnings)) = guard.as_ref()
            && cached_sig == &sig
        {
            return (cached.clone(), cached_warnings.clone(), true);
        }
        // 索引文件命中（D2/F10 接线）：签名与格式版本一致 → 免解析双快照，
        // 毫秒级恢复完整价格表（含分项可空语义与重建时诊断）。
        if let Ok(Some(index)) = load_index(index_path)
            && (INDEX_VERSION - 1..=INDEX_VERSION).contains(&index.v)
            && index.sig == sig
        {
            let arc = std::sync::Arc::new(Self::from_index(&index));
            *guard = Some((sig, arc.clone(), index.warnings.clone()));
            return (arc, index.warnings, true);
        }
        let (pricing, mut warnings) = Self::load(external, modelsdev_snapshot, openrouter_snapshot);
        // 重建后写索引快照，供下次进程启动快速加载。
        let index = pricing.to_index(
            sig.clone(),
            jiff::Zoned::now().to_string(),
            warnings.clone(),
        );
        if let Err(e) = save_index(index_path, &index) {
            warnings.push(format!("价格索引写入失败（不影响统计）: {e:#}"));
        }
        let arc = std::sync::Arc::new(pricing);
        *guard = Some((sig, arc.clone(), warnings.clone()));
        (arc, warnings, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用固定"事件时间"（历史时刻，绝不读当前墙上时钟）。
    fn at() -> jiff::Timestamp {
        "2026-01-05T10:00:00Z".parse().unwrap()
    }

    fn counts(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
        TokenCounts {
            input,
            output,
            cache_write: cw,
            cache_read: cr,
        }
    }

    fn write(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn test_pricing_normalize() {
        // Task 2A：归一化保留 vendor/ 渠道信息，匹配键另取末段。
        assert_eq!(
            normalize_model_id("Anthropic/Claude-Sonnet-4.5"),
            "anthropic/claude-sonnet-4-5"
        );
        assert_eq!(
            normalize_model_id("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5-20250929"
        );
        assert_eq!(normalize_model_id("Tencent/HY3:free"), "tencent/hy3:free");
        assert_eq!(normalize_model_id("GPT-5.6-Sol"), "gpt-5-6-sol");
        // 末段提取：最后一个 `/` 之后。
        assert_eq!(
            match_key("nano-gpt/qwen/qwen3.8-27b:thinking"),
            "qwen3-8-27b:thinking"
        );
        assert_eq!(
            match_key("anthropic/claude-sonnet-4.5"),
            "claude-sonnet-4-5"
        );
        assert_eq!(match_key("claude-sonnet-4-5"), "claude-sonnet-4-5");
    }

    #[test]
    fn test_pricing_known_model() {
        let p = fixture_pricing()
            .lookup("claude-sonnet-4-5-20250929")
            .unwrap();
        assert_eq!(p.plan.base.input, Some(3.0));
        assert_eq!(p.plan.base.output, Some(15.0));
        let p = fixture_pricing()
            .lookup("Claude-Sonnet-4-20250514")
            .unwrap();
        assert_eq!(p.plan.base.input, Some(3.0));
    }

    #[test]
    fn test_pricing_longest_prefix() {
        let p = fixture_pricing().lookup("gpt-5.6-luna").unwrap();
        assert_eq!(p.plan.base.input, Some(0.2));
        assert_eq!(
            fixture_pricing()
                .lookup("gpt-5.6-sol")
                .unwrap()
                .plan
                .base
                .input,
            Some(4.0)
        );
        assert_eq!(
            fixture_pricing()
                .lookup("gpt-5.4-nano")
                .unwrap()
                .plan
                .base
                .input,
            Some(0.2)
        );
        assert_eq!(
            fixture_pricing()
                .lookup("claude-opus-4-5-20251101")
                .unwrap()
                .plan
                .base
                .input,
            Some(5.0)
        );
        assert_eq!(
            fixture_pricing()
                .lookup("claude-opus-4-20250514")
                .unwrap()
                .plan
                .base
                .input,
            Some(15.0)
        );
        assert_eq!(
            fixture_pricing()
                .lookup("deepseek-v4-flash-0731")
                .unwrap()
                .plan
                .base
                .input,
            Some(0.3)
        );
        assert_eq!(
            fixture_pricing()
                .lookup("grok-4.5-build")
                .unwrap()
                .plan
                .base
                .input,
            Some(2.0)
        );
        assert_eq!(
            fixture_pricing()
                .lookup("grok-4-1-fast-reasoning")
                .unwrap()
                .plan
                .base
                .input,
            Some(0.2)
        );
    }

    #[test]
    fn test_pricing_normalize_matches_dot_naming() {
        // OpenRouter 点号命名经归一化后命中 fixture 表（反之亦然）。
        assert_eq!(
            fixture_pricing()
                .lookup("anthropic/claude-sonnet-4.5")
                .unwrap()
                .plan
                .base
                .input,
            Some(3.0)
        );
    }

    #[test]
    fn test_fixture_price_columns_are_not_swapped() {
        // Task 4（P1）：价格表 (prefix, input, output, cache_write, cache_read)，
        // gpt/grok 家族此前把缓存读价填进了缓存写列。逐字段直接断言
        // ModelPrice（不经费用反推），证据 = models.dev 权威快照 + OpenAI
        // prompt-caching 文档（写 1.25×、读 0.1×）。
        let gpt56 = fixture_pricing().lookup("gpt-5.6-sol").unwrap();
        assert_eq!(gpt56.plan.base.input, Some(4.0));
        assert_eq!(gpt56.plan.base.output, Some(20.0));
        assert_eq!(
            gpt56.plan.base.cache_write,
            Some(5.0),
            "缓存写 = 1.25×input"
        );
        assert_eq!(gpt56.plan.base.cache_read, Some(0.4), "缓存读 = 0.1×input");
        let luna = fixture_pricing().lookup("gpt-5.6-luna").unwrap();
        assert_eq!(luna.plan.base.cache_write, Some(0.25));
        assert_eq!(luna.plan.base.cache_read, Some(0.02));
        let gpt55 = fixture_pricing().lookup("gpt-5.5").unwrap();
        assert_eq!(
            gpt55.plan.base.cache_write,
            Some(0.0),
            "GPT-5.6 之前无写计费"
        );
        assert_eq!(
            gpt55.plan.base.cache_read,
            Some(0.5),
            "读价在第三列被填反过"
        );
        let grok4 = fixture_pricing().lookup("grok-4").unwrap();
        assert_eq!(grok4.plan.base.cache_write, Some(0.0));
        assert_eq!(grok4.plan.base.cache_read, Some(0.75));
        // Claude 家族顺序一直正确（对照 aihubmix/claude-opus-4-5）。
        let opus = fixture_pricing().lookup("claude-opus-4-5").unwrap();
        assert_eq!(opus.plan.base.cache_write, Some(6.25));
        assert_eq!(opus.plan.base.cache_read, Some(0.5));
    }

    #[test]
    fn test_fixture_gpt_cache_read_write_values() {
        // 费率结构防线：无写计费家族 cr = 0.1×input 且 cw = 0；
        // 5.6+ 家族 cw = 1.25×input 且 cr = 0.1×input。
        let no_write = [
            ("gpt-5.5", 5.0),
            ("gpt-5.4", 2.5),
            ("gpt-5.3-codex", 1.75),
            ("gpt-5.2", 1.75),
            ("gpt-5.1", 1.25),
            ("gpt-5", 1.25),
            ("gpt-5-mini", 0.25),
            ("gpt-5-nano", 0.05),
        ];
        for (prefix, input) in no_write {
            let p = fixture_pricing().lookup(prefix).unwrap();
            assert_eq!(p.plan.base.cache_write, Some(0.0), "{prefix} 无写计费");
            assert!(
                p.plan
                    .base
                    .cache_read
                    .resolve_direct()
                    .is_some_and(|v| (v - 0.1 * input).abs() < 1e-9),
                "{prefix} 缓存读 = 0.1×input"
            );
        }
        for (prefix, input) in [
            ("gpt-5.6", 4.0),
            ("gpt-5.6-luna", 0.2),
            ("gpt-5.6-terra", 2.0),
        ] {
            let p = fixture_pricing().lookup(prefix).unwrap();
            assert!(
                p.plan
                    .base
                    .cache_write
                    .resolve_direct()
                    .is_some_and(|v| (v - 1.25 * input).abs() < 1e-9),
                "{prefix} 写 = 1.25×input"
            );
            assert!(
                p.plan
                    .base
                    .cache_read
                    .resolve_direct()
                    .is_some_and(|v| (v - 0.1 * input).abs() < 1e-9),
                "{prefix} 读 = 0.1×input"
            );
        }
    }

    #[test]
    fn test_fixture_lookup_longest_prefix_boundary() {
        // Task 4：前缀命中必须有词元边界——"gpt-50" 不得命中 "gpt-5"。
        assert!(fixture_pricing().lookup("gpt-50").is_none());
        assert!(fixture_pricing().lookup("gpt-50-mini").is_none());
        // 合法前缀不受影响（分隔符后缀仍命中）。
        assert!(fixture_pricing().lookup("gpt-5.6-sol").is_some());
        assert!(
            fixture_pricing()
                .lookup("claude-sonnet-4-5-20250929")
                .is_some()
        );
        assert!(fixture_pricing().lookup("grok-4.20-0309").is_some());
    }

    /// Task 1：内置表已删除——测试用 fixture 表替代（tier 走 models.dev，
    /// 语义与主源一致；覆盖旧测试引用的全部模型）。
    fn fixture_pricing() -> Pricing {
        let mut p = Pricing::empty();
        for (prefix, i, o, cw, cr) in [
            ("claude-opus-5", 5.0, 25.0, Some(6.25), Some(0.5)),
            ("claude-opus-4-5", 5.0, 25.0, Some(6.25), Some(0.5)),
            ("claude-opus-4", 15.0, 75.0, Some(18.75), Some(1.5)),
            ("claude-sonnet-4-5", 3.0, 15.0, Some(3.75), Some(0.3)),
            ("claude-sonnet-4", 3.0, 15.0, Some(3.75), Some(0.3)),
            ("deepseek-v4-flash", 0.3, 1.2, Some(0.006), Some(0.0)),
            ("grok-4.5", 2.0, 6.0, Some(0.3), None),
            ("grok-4-1-fast", 0.2, 0.5, Some(0.05), None),
            ("gpt-5.6", 4.0, 20.0, Some(5.0), Some(0.4)),
            ("gpt-5.6-luna", 0.2, 1.2, Some(0.25), Some(0.02)),
            ("gpt-5.6-terra", 2.0, 12.0, Some(2.5), Some(0.2)),
            ("gpt-5.5", 5.0, 30.0, Some(0.0), Some(0.5)),
            ("gpt-5.4-nano", 0.2, 1.25, Some(0.0), Some(0.02)),
            ("gpt-5.4", 2.5, 15.0, Some(0.0), Some(0.25)),
            ("gpt-5.3-codex", 1.75, 14.0, Some(0.0), Some(0.175)),
            ("gpt-5.2", 1.75, 14.0, Some(0.0), Some(0.175)),
            ("gpt-5.1", 1.25, 10.0, Some(0.0), Some(0.125)),
            ("gpt-5-mini", 0.25, 2.0, Some(0.0), Some(0.025)),
            ("gpt-5-nano", 0.05, 0.4, Some(0.0), Some(0.005)),
            ("gpt-5", 1.25, 10.0, Some(0.0), Some(0.125)),
            ("grok-4", 3.0, 15.0, Some(0.0), Some(0.75)),
            ("grok-4.20", 1.25, 2.5, Some(0.0), Some(0.2)),
            ("grok-3-mini", 0.25, 0.5, Some(0.0), Some(0.075)),
            ("grok-3", 3.0, 15.0, Some(0.0), Some(0.75)),
        ] {
            p.add_entry(Entry {
                prefix: normalize_model_id(prefix),
                display: prefix.to_string(),
                name: None,
                plan: PricePlan {
                    base: PriceRates {
                        input: RateSpec::Fixed(i),
                        output: RateSpec::Fixed(o),
                        cache_write: RateSpec::fixed_or_unknown(cw),
                        cache_read: RateSpec::fixed_or_unknown(cr),
                    },
                    ..Default::default()
                },
                tier: TIER_MODELSDEV,
            });
        }
        p
    }

    /// Task 1 fixture：三段上下文价格计划（Claude 1M 长上下文样式）。
    fn tiered_plan() -> PricePlan {
        PricePlan {
            basis: Some(PricingBasis::PromptTokens),
            application: Some(PricingApplication::WholeRequest),
            schedules: Vec::new(),
            base: PriceRates {
                input: RateSpec::Fixed(3.0),
                output: RateSpec::Fixed(15.0),
                cache_write: RateSpec::Fixed(3.75),
                cache_read: RateSpec::Fixed(0.3),
            },
            segments: vec![
                PriceSegment {
                    label: Some("long".into()),
                    min_tokens: 200_000,
                    max_tokens: Some(1_000_000),
                    prices: PriceRates {
                        input: RateSpec::Fixed(6.0),
                        output: RateSpec::Fixed(22.5),
                        cache_write: RateSpec::Fixed(7.5),
                        cache_read: RateSpec::Fixed(0.6),
                    },
                },
                PriceSegment {
                    label: Some("ultra".into()),
                    min_tokens: 1_000_000,
                    max_tokens: None,
                    prices: PriceRates {
                        input: RateSpec::Fixed(9.0),
                        output: RateSpec::Fixed(30.0),
                        cache_write: RateSpec::Fixed(11.25),
                        cache_read: RateSpec::Fixed(0.9),
                    },
                },
            ],
        }
    }

    #[test]
    fn test_pricing_segment_selection_boundaries() {
        let plan = tiered_plan();
        // 下界、界内、上界前、最后一档。
        assert!(select_segment(&plan, 0).is_none(), "低于第一段 → 基础价");
        assert!(select_segment(&plan, 200_000).is_some(), "[min, max) 左闭");
        assert!(select_segment(&plan, 999_999).is_some());
        assert_eq!(
            select_segment(&plan, 1_000_000).unwrap().label.as_deref(),
            Some("ultra")
        );
        assert_eq!(
            select_segment(&plan, 1_000_001).unwrap().label.as_deref(),
            Some("ultra"),
            "无上限段覆盖之后所有 basis 值"
        );
        // 计划 fixture：三段 [0,100000) [100000,272001) [272001,None)。
        let plan3 = PricePlan {
            base: PriceRates {
                input: RateSpec::Fixed(1.0),
                ..Default::default()
            },
            segments: vec![
                PriceSegment {
                    label: Some("short".into()),
                    min_tokens: 0,
                    max_tokens: Some(100_000),
                    prices: PriceRates {
                        input: RateSpec::Fixed(2.0),
                        ..Default::default()
                    },
                },
                PriceSegment {
                    label: Some("mid".into()),
                    min_tokens: 100_000,
                    max_tokens: Some(272_001),
                    prices: PriceRates {
                        input: RateSpec::Fixed(4.0),
                        ..Default::default()
                    },
                },
                PriceSegment {
                    label: Some("long".into()),
                    min_tokens: 272_001,
                    max_tokens: None,
                    prices: PriceRates {
                        input: RateSpec::Fixed(8.0),
                        ..Default::default()
                    },
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            select_segment(&plan3, 0).unwrap().label.as_deref(),
            Some("short"),
            "下界 0 命中第一段"
        );
        assert_eq!(
            select_segment(&plan3, 99_999).unwrap().label.as_deref(),
            Some("short")
        );
        assert_eq!(
            select_segment(&plan3, 100_000).unwrap().label.as_deref(),
            Some("mid"),
            "边界 100000 只命中后一段"
        );
        assert_eq!(
            select_segment(&plan3, 272_000).unwrap().label.as_deref(),
            Some("mid")
        );
        assert_eq!(
            select_segment(&plan3, 272_001).unwrap().label.as_deref(),
            Some("long"),
            "272001 命中最后一档"
        );
        assert_eq!(
            select_segment(&plan3, u64::MAX).unwrap().label.as_deref(),
            Some("long"),
            "最后一档覆盖其后所有值"
        );
    }

    #[test]
    fn test_pricing_segment_validation_rejects_bad_rules() {
        let mut plan = tiered_plan();
        // 重叠：第二段 min 低于第一段 max。
        plan.segments[1].min_tokens = 500_000;
        assert!(validate_price_plan(&plan).is_err());
        // 反向：max <= min。
        let mut plan = tiered_plan();
        plan.segments[0].max_tokens = Some(100_000);
        assert!(validate_price_plan(&plan).is_err());
        // 空洞：第二段 min > 第一段 max。
        let mut plan = tiered_plan();
        plan.segments[1].min_tokens = 2_000_000;
        assert!(validate_price_plan(&plan).is_err());
        // 非单调：分段乱序（无上限段在前）。
        let mut plan = tiered_plan();
        plan.segments.swap(0, 1);
        assert!(validate_price_plan(&plan).is_err());
        // 无上限段后不允许再有分段。
        let mut plan = tiered_plan();
        plan.segments.push(PriceSegment {
            label: Some("after".into()),
            min_tokens: 2_000_000,
            max_tokens: None,
            prices: PriceRates::default(),
        });
        assert!(validate_price_plan(&plan).is_err());
        // 负价格。
        let mut plan = tiered_plan();
        plan.base.input = RateSpec::Fixed(-1.0);
        assert!(validate_price_plan(&plan).is_err());
        // 分段内负价格同样拒绝。
        let mut plan = tiered_plan();
        plan.segments[0].prices.cache_read = RateSpec::Fixed(-0.5);
        assert!(validate_price_plan(&plan).is_err());
        // 合法计划通过。
        assert!(validate_price_plan(&tiered_plan()).is_ok());
    }

    #[test]
    fn test_pricing_segment_effective_rates_fallback() {
        // 分段只覆盖 input：output/cache 回退基础价；显式 0 不回退。
        let plan = PricePlan {
            basis: Some(PricingBasis::PromptTokens),
            application: Some(PricingApplication::WholeRequest),
            base: PriceRates {
                input: RateSpec::Fixed(3.0),
                output: RateSpec::Fixed(15.0),
                cache_write: RateSpec::Fixed(3.75),
                cache_read: RateSpec::Fixed(0.3),
            },
            schedules: Vec::new(),
            segments: vec![PriceSegment {
                label: Some("input-only".into()),
                min_tokens: 200_000,
                max_tokens: None,
                prices: PriceRates {
                    input: RateSpec::Fixed(6.0),
                    output: RateSpec::Unknown,         // 沿用基础 15.0
                    cache_write: RateSpec::Fixed(0.0), // 显式免费，不回退
                    cache_read: RateSpec::Unknown,
                },
            }],
        };
        let seg = select_segment(&plan, 300_000).unwrap();
        let r = effective_rates(&plan, None, Some(seg));
        assert_eq!(r.input, Some(6.0));
        assert_eq!(r.output, Some(15.0), "分段缺 output → 沿用基础");
        assert_eq!(r.cache_write, Some(0.0), "显式 0 不回退");
        assert_eq!(r.cache_read, Some(0.3));
        // 基础价也缺失的分量 → None（估算时进 unknown）。
        let plan2 = PricePlan {
            base: PriceRates {
                input: RateSpec::Fixed(3.0),
                ..Default::default()
            },
            segments: vec![],
            ..Default::default()
        };
        let r2 = effective_rates(&plan2, None, None);
        assert_eq!(r2.input, Some(3.0));
        assert_eq!(r2.output, None, "无基础价 → unknown");
    }

    #[test]
    fn test_pricing_segment_estimate_end_to_end() {
        // 无来源入口时 estimate 走分段：basis = prompt = input + cw + cr。
        let mut p = Pricing::empty();
        p.add_entry(Entry {
            prefix: normalize_model_id("tiered-model"),
            display: "tiered-model".into(),
            name: None,
            plan: tiered_plan(),
            tier: TIER_MODELSDEV,
        });
        let counts = |input: u64, cw: u64, cr: u64, output: u64| TokenCounts {
            input,
            output,
            cache_write: cw,
            cache_read: cr,
        };
        // 低于阈值 → 基础价：200_000 前的 (100, 0, 0, 50)。
        let e1 = p
            .estimate("tiered-model", &counts(100, 0, 0, 50), at())
            .unwrap();
        assert!((e1.cost - (100.0 * 3.0 + 50.0 * 15.0) / 1e6).abs() < 1e-12);
        // 恰好等于阈值 200_000 → 命中 long 段：prompt = 200_000。
        let e2 = p
            .estimate(
                "tiered-model",
                &counts(100_000, 50_000, 50_000, 1_000),
                at(),
            )
            .unwrap();
        assert!(
            (e2.cost - (100_000.0 * 6.0 + 1_000.0 * 22.5 + 50_000.0 * 7.5 + 50_000.0 * 0.6) / 1e6)
                .abs()
                < 1e-9
        );
        // 1_000_000 → ultra 段。
        let e3 = p
            .estimate("tiered-model", &counts(1_000_000, 0, 0, 10), at())
            .unwrap();
        assert!((e3.cost - (1_000_000.0 * 9.0 + 10.0 * 30.0) / 1e6).abs() < 1e-9);
    }

    #[test]
    fn test_pricing_segment_no_rate_is_unknown_not_zero() {
        // 无基础价且未命中分段：token 进 unknown（complete=false），
        // 不按 0 计费（不变量 5/6）。
        let mut p = Pricing::empty();
        p.add_entry(Entry {
            prefix: normalize_model_id("no-base-model"),
            display: "no-base-model".into(),
            name: None,
            plan: PricePlan {
                basis: Some(PricingBasis::PromptTokens),
                application: Some(PricingApplication::WholeRequest),
                base: PriceRates::default(),
                schedules: Vec::new(),
                segments: vec![PriceSegment {
                    label: Some(">200K".into()),
                    min_tokens: 200_000,
                    max_tokens: None,
                    prices: PriceRates {
                        input: RateSpec::Fixed(6.0),
                        output: RateSpec::Fixed(22.5),
                        cache_write: RateSpec::Fixed(7.5),
                        cache_read: RateSpec::Fixed(0.6),
                    },
                }],
            },
            tier: TIER_MODELSDEV,
        });
        let e = p
            .estimate("no-base-model", &counts(100, 10, 0, 0), at())
            .unwrap();
        assert!(!e.complete, "缺价且未命中分段 → 不完整，不得当免费");
        assert_eq!(e.unknown.input, 100);
        assert_eq!(e.unknown.output, 10);
        assert_eq!(e.cost, 0.0);
        // 命中分段后正常计价。
        let e2 = p
            .estimate("no-base-model", &counts(200_000, 0, 0, 0), at())
            .unwrap();
        assert!(e2.complete, "命中分段且有价 → 完整");
        assert!((e2.cost - 200_000.0 * 6.0 / 1e6).abs() < 1e-12);
    }

    /// Task 2A fixture：同末段模型名的两个渠道候选（A 贵、B 便宜）。
    fn candidate_pricing() -> Pricing {
        let mut p = Pricing::empty();
        for (raw, i, o, cw, cr) in [
            (
                "nano-gpt/qwen/qwen3.8-27b-obliterated:thinking",
                8.0,
                40.0,
                10.0,
                0.8,
            ),
            (
                "other-channel/qwen/qwen3.8-27b-obliterated:thinking",
                2.0,
                10.0,
                2.5,
                0.2,
            ),
        ] {
            p.add_entry(Entry {
                prefix: match_key(raw),
                display: raw.to_string(),
                name: None,
                plan: PricePlan {
                    base: PriceRates {
                        input: RateSpec::Fixed(i),
                        output: RateSpec::Fixed(o),
                        cache_write: RateSpec::Fixed(cw),
                        cache_read: RateSpec::Fixed(cr),
                    },
                    ..Default::default()
                },
                tier: TIER_OPENROUTER,
            });
        }
        p
    }

    #[test]
    fn test_pricing_candidate_selection() {
        // 1) 两个渠道末段完整相同，候选都能被找到；本请求条件下最高费用
        //    的渠道 A 胜出（保守估算），元数据可解释选择原因。
        let p = candidate_pricing();
        let est = p
            .estimate(
                "nano-gpt/qwen/qwen3.8-27b-obliterated:thinking",
                &counts(1_000_000, 0, 0, 0),
                at(),
            )
            .unwrap();
        let m = est.matched.as_ref().expect("命中必须有元数据");
        assert_eq!(m.candidate_count, 2, "两个渠道都进入候选");
        assert_eq!(m.raw_key, "nano-gpt/qwen/qwen3.8-27b-obliterated:thinking");
        assert_eq!(m.channel.as_deref(), Some("nano-gpt"));
        assert_eq!(m.source, "openrouter");
        assert_eq!(m.match_mode, MatchMode::Full);
        assert_eq!(m.reason, "candidates_highest_cost");
        assert_eq!(m.matched_key, "qwen3-8-27b-obliterated:thinking");
        assert!((est.cost - 8.0).abs() < 1e-9);
        // 查询另一渠道：同一候选组，仍是 A 胜出。
        let est = p
            .estimate(
                "other-channel/qwen/qwen3.8-27b-obliterated:thinking",
                &counts(1_000_000, 0, 0, 0),
                at(),
            )
            .unwrap();
        assert_eq!(
            est.matched.as_ref().unwrap().raw_key,
            "nano-gpt/qwen/qwen3.8-27b-obliterated:thinking"
        );

        // 2) 候选按请求条件（basis token + 分段）逐一计价后裁决：
        //    小请求平价贵者胜，大请求分段渠道反超。
        let mut p2 = Pricing::empty();
        p2.add_entry(Entry {
            prefix: match_key("chan-a/mix"),
            display: "chan-a/mix".into(),
            name: None,
            plan: PricePlan {
                base: PriceRates {
                    input: RateSpec::Fixed(8.0),
                    ..Default::default()
                },
                ..Default::default()
            },
            tier: TIER_OPENROUTER,
        });
        p2.add_entry(Entry {
            prefix: match_key("chan-b/mix"),
            display: "chan-b/mix".into(),
            name: None,
            plan: PricePlan {
                base: PriceRates {
                    input: RateSpec::Fixed(2.0),
                    ..Default::default()
                },
                segments: vec![PriceSegment {
                    label: Some(">100K".into()),
                    min_tokens: 100_000,
                    max_tokens: None,
                    prices: PriceRates {
                        input: RateSpec::Fixed(20.0),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            tier: TIER_OPENROUTER,
        });
        let est = p2
            .estimate("chan-a/mix", &counts(50_000, 0, 0, 0), at())
            .unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-a/mix");
        assert!((est.cost - 50_000.0 * 8.0 / 1e6).abs() < 1e-9);
        let est = p2
            .estimate("chan-a/mix", &counts(200_000, 0, 0, 0), at())
            .unwrap();
        assert_eq!(
            est.matched.as_ref().unwrap().raw_key,
            "chan-b/mix",
            "大请求时分段渠道更贵 → 保守取 B"
        );
        assert!((est.cost - 200_000.0 * 20.0 / 1e6).abs() < 1e-9);

        // 3) 完整匹配优先于前缀匹配（分阶段：完整命中时前缀不参与）。
        let mut p3 = Pricing::empty();
        for (key, disp) in [
            (
                "qwen3-8-27b-obliterated:thinking",
                "qwen3.8-27b-obliterated:thinking",
            ),
            ("qwen3-8-27b", "qwen3.8-27b"),
        ] {
            p3.add_entry(Entry {
                prefix: key.to_string(),
                display: disp.to_string(),
                name: None,
                plan: PricePlan {
                    base: PriceRates {
                        input: RateSpec::Fixed(3.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                tier: TIER_OPENROUTER,
            });
        }
        let est = p3
            .estimate(
                "qwen3.8-27b-obliterated:thinking",
                &counts(1_000, 0, 0, 0),
                at(),
            )
            .unwrap();
        let m = est.matched.as_ref().unwrap();
        assert_eq!(m.matched_key, "qwen3-8-27b-obliterated:thinking");
        assert_eq!(m.match_mode, MatchMode::Full);

        // 4) 有边界前缀：`qwen3.8-27b` 命中 `qwen3.8-27b-instruct`，
        //    不得命中 `qwen3.8-27b2`。
        let est = p3
            .estimate("qwen3.8-27b-instruct", &counts(1_000, 0, 0, 0), at())
            .unwrap();
        let m = est.matched.as_ref().unwrap();
        assert_eq!(m.matched_key, "qwen3-8-27b");
        assert_eq!(m.match_mode, MatchMode::Prefix);
        assert!(
            p3.estimate("qwen3.8-27b2", &counts(1_000, 0, 0, 0), at())
                .is_none(),
            "无词元边界的前缀不得命中"
        );

        // 5) 命中元数据可序列化（tooltip/breakdown 载体）。
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"matched_key\":\"qwen3-8-27b\""));
        assert!(json.contains("\"match_mode\":\"prefix\""));
        assert!(json.contains("\"source\":\"openrouter\""));
    }

    #[test]
    fn test_pricing_modelsdev_tier() {
        // Task 2：快照分段 → PricePlan；合法规则参与计价，非法规则防御性
        // 丢弃分段只留基础价并给出诊断。
        use crate::modelsdev::{Snapshot, SnapshotEntry, SnapshotSegment};
        let dir = std::env::temp_dir().join(format!("tokenscope-pmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = dir.join("pricing-modelsdev.json");
        let seg = |min: u64, max: Option<u64>, input: f64| SnapshotSegment {
            label: Some(format!(">{min}")),
            min_tokens: min,
            max_tokens: max,
            input: Some(input),
            output: Some(input * 4.0),
            cache_read: Some(input / 10.0),
            cache_write: Some(input * 1.25),
        };
        let snapshot = Snapshot {
            v: 3,
            synced_at: "t".into(),
            entries: vec![
                SnapshotEntry {
                    id: "prov/tiered".into(),
                    name: None,
                    input: Some(4.0),
                    output: Some(20.0),
                    cache_read: Some(0.4),
                    cache_write: Some(5.0),
                    segments: vec![seg(272_001, None, 8.0)],
                },
                // 反向区间：非法 → 仅保留基础价 + warning
                SnapshotEntry {
                    id: "prov/bad-seg".into(),
                    name: None,
                    input: Some(1.0),
                    output: Some(2.0),
                    cache_read: None,
                    cache_write: None,
                    segments: vec![seg(100_000, Some(50_000), 9.0)],
                },
            ],
        };
        std::fs::write(&snap, serde_json::to_string(&snapshot).unwrap()).unwrap();
        let (p, warnings) = Pricing::load(None, Some(&snap), None);
        assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
        assert!(warnings[0].contains("bad-seg"));
        // 272000 → 基础档；272001 → 高档。
        let est = p
            .estimate("prov/tiered", &counts(272_000, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 272_000.0 * 4.0 / 1e6).abs() < 1e-9);
        let est = p
            .estimate("prov/tiered", &counts(272_001, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 272_001.0 * 8.0 / 1e6).abs() < 1e-9);
        // 非法分段的条目仍按基础价计价。
        let est = p
            .estimate("prov/bad-seg", &counts(100_000, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 100_000.0 * 1.0 / 1e6).abs() < 1e-9);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_openrouter_override() {
        // Task 3：OpenRouter override 按 inclusive 下界命中——
        // 271999 基础价，272000 即命中第一档，1000000 命中第二档。
        use crate::openrouter::{Snapshot, SnapshotEntry, SnapshotOverride};
        let dir = std::env::temp_dir().join(format!("tokenscope-por-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = dir.join("pricing-openrouter.json");
        let ov = |min: u64, prompt: &str, cr: Option<f64>| SnapshotOverride {
            min_prompt_tokens: min,
            prompt: Some(prompt.parse().unwrap()),
            completion: Some(prompt.parse().unwrap()),
            cache_read: cr,
            cache_write: None,
        };
        let snapshot = Snapshot {
            v: 2,
            synced_at: "t".into(),
            entries: vec![SnapshotEntry {
                id: "prov/long".into(),
                name: None,
                prompt: Some(0.000004),
                completion: Some(0.00002),
                cache_read: None,
                cache_write: None,
                overrides: vec![
                    ov(272_000, "0.000006", Some(0.0000006)),
                    ov(1_000_000, "0.000008", None),
                ],
            }],
        };
        std::fs::write(&snap, serde_json::to_string(&snapshot).unwrap()).unwrap();
        let (p, warnings) = Pricing::load(None, None, Some(&snap));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        // 271999 → 基础档（4 USD/Mtok）。
        let est = p
            .estimate("prov/long", &counts(271_999, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 271_999.0 * 4.0 / 1e6).abs() < 1e-9);
        // 272000 → 第一档（6 USD/Mtok，inclusive 下界）。
        let est = p
            .estimate("prov/long", &counts(272_000, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 272_000.0 * 6.0 / 1e6).abs() < 1e-9);
        // 999_999 → 仍第一档。
        let est = p
            .estimate("prov/long", &counts(999_999, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 999_999.0 * 6.0 / 1e6).abs() < 1e-9);
        // 1_000_000 → 第二档（8 USD/Mtok）；cache_read 第二档缺价 →
        // unknown 不按 0（分项 token 为 0 时不影响完整性）。
        let est = p
            .estimate("prov/long", &counts(1_000_000, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 1_000_000.0 * 8.0 / 1e6).abs() < 1e-9);
        assert!(est.complete);
        // 第二档带 cache_read token：第一档有价（0.6 USD/Mtok），第二档缺价
        // → 部分 unknown。
        let est = p
            .estimate("prov/long", &counts(1_000_000, 0, 500, 0), at())
            .unwrap();
        assert!(!est.complete, "高档缺缓存读价 → 部分计价");
        assert_eq!(est.unknown.cache_write, 500);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_unknown_model() {
        assert!(fixture_pricing().lookup("tencent/hy3:free").is_none());
        assert!(fixture_pricing().lookup("<synthetic>").is_none());
        assert!(
            fixture_pricing()
                .estimate("qwen-x", &counts(1, 1, 0, 0), at())
                .is_none()
        );
    }

    #[test]
    fn test_pricing_missing_component_not_free() {
        // B3（F04）：models.dev cost 缺键 = 分项未知，不按 0——
        // 非零缓存 token 不得被当成免费；显式 0 才是真免费。
        use crate::modelsdev::{Snapshot, SnapshotEntry};
        let dir = std::env::temp_dir().join(format!("tokenscope-b3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = dir.join("pricing-modelsdev.json");
        let snapshot = Snapshot {
            v: 2,
            synced_at: "t".into(),
            entries: vec![
                // 缺 cache_write/cache_read 键（serde None）
                SnapshotEntry {
                    id: "prov/partial".into(),
                    name: None,
                    input: Some(1.0),
                    output: Some(2.0),
                    cache_read: None,
                    cache_write: None,
                    segments: Vec::new(),
                },
                // 显式 0（如旧版 OpenAI 模型无写入计费）
                SnapshotEntry {
                    id: "prov/free-cache".into(),
                    name: None,
                    input: Some(1.0),
                    output: Some(2.0),
                    cache_read: Some(0.0),
                    cache_write: Some(0.0),
                    segments: Vec::new(),
                },
            ],
        };
        std::fs::write(&snap, serde_json::to_string(&snapshot).unwrap()).unwrap();
        let (p, w) = Pricing::load(None, Some(&snap), None);
        assert!(w.is_empty());

        // 部分计价：input/output 已知，cache 未知 → cost 只含已知部分。
        let est = p
            .estimate(
                "prov/partial",
                &counts(1_000_000, 1_000_000, 500, 700),
                at(),
            )
            .unwrap();
        assert!((est.cost - 3.0).abs() < 1e-9, "只计已知分项: {est:?}");
        assert!(!est.complete);
        assert_eq!(est.unknown.input, 0);
        assert_eq!(est.unknown.output, 0);
        assert_eq!(est.unknown.cache_write, 500);
        assert_eq!(est.unknown.cache_read, 700);

        // 显式零：缓存分项按 0 计价，complete=true。
        let est = p
            .estimate("prov/free-cache", &counts(1_000_000, 0, 500, 700), at())
            .unwrap();
        assert!((est.cost - 1.0).abs() < 1e-9);
        assert!(est.complete, "显式 0 不是未知: {est:?}");
        assert_eq!(est.unknown.total(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 全局 PRICE_CACHE 是共享态，三个索引/缓存测试必须串行。
    static CACHE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_pricing_warning_survives_cache_hit() {
        // D2：缓存命中也要携带诊断——降级状态随结果持续可见（不变量 11）。
        let dir = std::env::temp_dir().join(format!("tokenscope-d2-warn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 坏外置文件 → 加载警告
        let bad = dir.join("pricing.toml");
        std::fs::write(&bad, "not valid toml [[[").unwrap();
        let idx = dir.join("pricing-index.json");
        let _g = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (_, w1, hit1) = Pricing::load_cached(Some(&bad), None, None, &idx);
        assert!(!hit1);
        assert!(!w1.is_empty(), "重建路径应有警告");
        // 进程内缓存命中：警告不丢
        let (_, w2, hit2) = Pricing::load_cached(Some(&bad), None, None, &idx);
        assert!(hit2);
        assert_eq!(w2, w1, "缓存命中的警告必须保持");
        // 模拟重启（清进程缓存）：索引命中，警告随索引持久化
        *PRICE_CACHE.lock().unwrap() = None;
        let (_, w3, hit3) = Pricing::load_cached(Some(&bad), None, None, &idx);
        assert!(hit3, "索引应命中");
        assert_eq!(w3, w1, "索引命中的警告必须保持");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_version_invalidates_index() {
        // D2：版本不符的旧索引按失效重建（不因签名相同而命中）。
        let dir = std::env::temp_dir().join(format!("tokenscope-d2-ver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let idx = dir.join("pricing-index.json");
        let _g = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *PRICE_CACHE.lock().unwrap() = None;
        let (_, _, _) = Pricing::load_cached(None, None, None, &idx);
        *PRICE_CACHE.lock().unwrap() = None;
        // 篡改版本号为 1（旧格式）
        let mut index: PricingIndex =
            serde_json::from_str(&std::fs::read_to_string(&idx).unwrap()).unwrap();
        index.v = 1;
        std::fs::write(&idx, serde_json::to_string(&index).unwrap()).unwrap();
        let (_, _, hit) = Pricing::load_cached(None, None, None, &idx);
        assert!(!hit, "版本不符必须重建");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Task 5 fixture：partial 模型（input/output 有价，cache 分项未知）。
    fn partial_pricing_fixture(dir: &std::path::Path) -> Pricing {
        use crate::modelsdev::{Snapshot, SnapshotEntry};
        let snap = dir.join("pricing-modelsdev.json");
        let snapshot = Snapshot {
            v: 2,
            synced_at: "t".into(),
            entries: vec![SnapshotEntry {
                id: "prov/partial".into(),
                name: None,
                input: Some(1.0),
                output: Some(2.0),
                cache_read: None,
                cache_write: None,
                segments: Vec::new(),
            }],
        };
        std::fs::write(&snap, serde_json::to_string(&snapshot).unwrap()).unwrap();
        let (p, w) = Pricing::load(None, Some(&snap), None);
        assert!(w.is_empty());
        p
    }

    #[test]
    fn test_zero_token_unknown_price_is_complete() {
        // Task 5（P1）：未知分项 token 为 0 时不得标记不完整——
        // 修复前 cw/cr 缺价 + 0 用量也会打 † 。
        let dir = std::env::temp_dir().join(format!("tokenscope-t5-z-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = partial_pricing_fixture(&dir);
        let est = p
            .estimate("prov/partial", &counts(1_000_000, 1_000_000, 0, 0), at())
            .unwrap();
        assert!(est.complete, "零 token 的未知分项不影响完整性: {est:?}");
        assert_eq!(est.unknown.total(), 0);
        assert!((est.cost - 3.0).abs() < 1e-9);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_positive_token_unknown_price_is_partial() {
        let dir = std::env::temp_dir().join(format!("tokenscope-t5-p-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = partial_pricing_fixture(&dir);
        let est = p
            .estimate(
                "prov/partial",
                &counts(1_000_000, 1_000_000, 500, 700),
                at(),
            )
            .unwrap();
        assert!(!est.complete, "正 token 的未知分项必须标记部分计价");
        assert_eq!(est.unknown.cache_write, 500);
        assert_eq!(est.unknown.cache_read, 700);
        assert!((est.cost - 3.0).abs() < 1e-9, "费用仅含已计价部分");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_legacy_index_v2_with_builtin_entries_is_invalidated() {
        let _g = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Task 3（审阅）：旧索引测试曾用必然不匹配的 stale-sig——重建可能
        // 是签名失配而非版本失效（假阳性）。现改用生产签名函数构造 v2
        // 索引：签名与当前 load_cached 完全一致，仅版本过期，从而证明
        // 重建只由版本检查触发。
        let dir = std::env::temp_dir().join(format!("tokenscope-t5-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let idx = dir.join("pricing-index.json");
        // 生产同款签名：load_cached(None, None, None, _) 的三源签名。
        let sig = source_sig(&[None, None, None]);
        let legacy = serde_json::json!({
            "v": 2,
            "sig": sig,
            "synced_at": "2026-10-05T00:00:00Z",
            "entries": [
                { "prefix": "legacy-builtin-model", "display": "legacy-builtin-model",
                  "name": null, "tier": 3, "input": 1.0, "output": 2.0,
                  "cache_write": 0.0, "cache_read": 0.0 }
            ]
        });
        std::fs::write(&idx, serde_json::to_string(&legacy).unwrap()).unwrap();

        // 第一步：证明 fixture 有效——若只有签名检查，旧条目会被命中。
        let restored = load_index(&idx).unwrap().unwrap();
        assert_eq!(restored.sig, sig, "签名必须与生产计算一致");
        let via_from_index = Pricing::from_index(&restored);
        assert!(
            via_from_index.lookup("legacy-builtin-model").is_some(),
            "fixture 有效性：无视版本检查时旧内置条目会被命中"
        );

        // 第二步：当前实现因版本过期重建，旧条目不存在。
        *PRICE_CACHE.lock().unwrap() = None;
        let (p, _, _) = Pricing::load_cached(None, None, None, &idx);
        assert!(
            p.lookup("legacy-builtin-model").is_none(),
            "版本失效必须阻止旧内置条目恢复"
        );
        assert!(p.lookup("claude-sonnet-4-5").is_none(), "无来源 → unknown");
        // 清空进程内缓存，避免污染并行的同签名测试（restart_hit）。
        *PRICE_CACHE.lock().unwrap() = None;
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_index_migration() {
        // Task 9：旧 v3 扁平索引（首斜杠剥前缀口径）按版本失效重建；
        // 旧快照作纯基础价离线可用；新索引恢复后保留分段；候选计价
        // 不跨来源拼价。
        let dir = std::env::temp_dir().join(format!("tokenscope-t9-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let idx = dir.join("pricing-index.json");
        let _g = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *PRICE_CACHE.lock().unwrap() = None;
        // 生产同款签名：证明重建只由版本检查触发（v3 < 当前 v4）。
        let sig = source_sig(&[None, None, None]);
        let v3 = serde_json::json!({
            "v": 3,
            "sig": sig,
            "synced_at": "2026-10-06T00:00:00Z",
            "entries": [
                { "prefix": "old-flat-model", "display": "old/flat-model",
                  "name": null, "tier": 1, "input": 1.0, "output": 2.0,
                  "cache_write": 0.0, "cache_read": 0.0 }
            ]
        });
        std::fs::write(&idx, serde_json::to_string(&v3).unwrap()).unwrap();
        let (_, _, hit) = Pricing::load_cached(None, None, None, &idx);
        assert!(!hit, "v3 扁平索引必须按版本失效重建（不猜测渠道键）");
        // 全局 PRICE_CACHE 是共享态：本测试结束前清空，避免污染同签名的
        // 其他密闭性测试（如 restart_hit 的"首次必重建"）。
        *PRICE_CACHE.lock().unwrap() = None;

        // 旧 models.dev v2 快照（无分段字段）→ 离线仅基础价可用。
        use crate::modelsdev::{Snapshot as MdSnap, SnapshotEntry as MdEntry};
        let md_path = dir.join("pricing-modelsdev.json");
        let md_snap = MdSnap {
            v: 2,
            synced_at: "t".into(),
            entries: vec![MdEntry {
                id: "prov/legacy".into(),
                name: None,
                input: Some(1.0),
                output: Some(3.0),
                cache_read: Some(0.1),
                cache_write: Some(0.2),
                segments: Vec::new(),
            }],
        };
        std::fs::write(&md_path, serde_json::to_string(&md_snap).unwrap()).unwrap();
        let (p_legacy, w) = Pricing::load(None, Some(&md_path), None);
        assert!(w.is_empty());
        let est = p_legacy
            .estimate("prov/legacy", &counts(1_000_000, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 1.0).abs() < 1e-9, "旧快照按基础价离线计价");
        assert_eq!(est.segment_label, None);

        // 新索引（v4）往返：segments 保留，估算命中高档。
        let mut p_new = Pricing::empty();
        p_new.add_entry(Entry {
            prefix: match_key("prov/seg"),
            display: "prov/seg".into(),
            name: None,
            plan: PricePlan {
                base: PriceRates {
                    input: RateSpec::Fixed(2.0),
                    ..Default::default()
                },
                segments: vec![PriceSegment {
                    label: Some(">100K".into()),
                    min_tokens: 100_001,
                    max_tokens: None,
                    prices: PriceRates {
                        input: RateSpec::Fixed(9.0),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            tier: TIER_MODELSDEV,
        });
        let index = p_new.to_index(sig, "t".into(), Vec::new());
        std::fs::write(&idx, serde_json::to_string(&index).unwrap()).unwrap();
        let restored = Pricing::from_index(&load_index(&idx).unwrap().unwrap());
        let est = restored
            .estimate("prov/seg", &counts(100_001, 0, 0, 0), at())
            .unwrap();
        assert_eq!(
            est.segment_label.as_deref(),
            Some(">100K"),
            "新索引恢复保留分段"
        );
        assert!((est.cost - 100_001.0 * 9.0 / 1e6).abs() < 1e-9);

        // 来源冲突不跨 provider 拼价：external（贵）与 models.dev（便宜）
        // 同名候选并存，命中行的 source 与四类单价必须同源。
        let ext = dir.join("pricing.toml");
        std::fs::write(
            &ext,
            "[[model]]
prefix = \"prov/seg\"
input = 20.0
output = 40.0
cache_write = 5.0
cache_read = 1.0
",
        )
        .unwrap();
        let (p_mix, wm) = Pricing::load(Some(&ext), Some(&md_path), None);
        assert!(wm.is_empty(), "warnings: {wm:?}");
        let est = p_mix
            .estimate("prov/seg", &counts(1_000_000, 1_000_000, 0, 0), at())
            .unwrap();
        let m = est.matched.as_ref().unwrap();
        assert_eq!(m.source, "external", "贵候选胜出");
        let ext_in = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::Input)
            .unwrap();
        assert_eq!(ext_in.unit_price, Some(20.0), "input 单价来自 external");
        let ext_out = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::Output)
            .unwrap();
        assert_eq!(
            ext_out.unit_price,
            Some(40.0),
            "output 单价同源，不从 models.dev 拼价"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_index_restart_hit() {
        // D2/F10 接线：索引文件签名一致 → 跳过双快照解析直接恢复。
        let dir = std::env::temp_dir().join(format!("tokenscope-b3-idx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let idx = dir.join("pricing-index.json");
        let _g = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (p1, _, hit1) = Pricing::load_cached(None, None, None, &idx);
        assert!(!hit1, "首次必重建");
        // 清空进程内缓存，模拟重启：签名一致 → 索引命中。
        *PRICE_CACHE.lock().unwrap() = None;
        let (p2, w2, hit2) = Pricing::load_cached(None, None, None, &idx);
        assert!(hit2, "索引文件应命中");
        assert!(w2.is_empty());
        assert_eq!(p2.all_entries().count(), p1.all_entries().count());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_cost_math() {
        let est = fixture_pricing()
            .estimate(
                "claude-sonnet-4-5",
                &counts(1_000_000, 1_000_000, 0, 0),
                at(),
            )
            .unwrap();
        assert!((est.cost - 18.0).abs() < 1e-9);
        assert!(est.complete);
        let est = fixture_pricing()
            .estimate(
                "claude-sonnet-4-5",
                &counts(0, 0, 1_000_000, 1_000_000),
                at(),
            )
            .unwrap();
        assert!((est.cost - 4.05).abs() < 1e-9);
        let est = fixture_pricing()
            .estimate("gpt-5.6-sol", &counts(800, 100, 50, 200), at())
            .unwrap();
        // 纯价格数学：桶值直接给定（不经适配器）。
        // Task 4 校正后：800*4 + 100*20 + 50*5.0(写) + 200*0.4(读)。
        assert!((est.cost - 5530.0 / 1_000_000.0).abs() < 1e-12);
    }

    #[test]
    fn test_pricing_entries_segments() {
        // Task 8：设置页条目携带原始键/渠道/来源/依据/分段/峰谷视图；
        // 缺失分项 = None（前端显示"未知"），显式 0 保留为 0。
        let mut p = Pricing::empty();
        // 分段条目：基础价缺 cache_read（None），显式 0 的 cache_write。
        p.add_entry(Entry {
            prefix: match_key("nano-gpt/qwen/tiered-view"),
            display: "nano-gpt/qwen/tiered-view".into(),
            name: Some("Tiered View".into()),
            plan: PricePlan {
                basis: Some(PricingBasis::PromptTokens),
                application: Some(PricingApplication::WholeRequest),
                base: PriceRates {
                    input: RateSpec::Fixed(4.0),
                    output: RateSpec::Fixed(20.0),
                    cache_write: RateSpec::Fixed(0.0),
                    cache_read: RateSpec::Unknown,
                },
                segments: vec![PriceSegment {
                    label: Some(">272K".into()),
                    min_tokens: 272_001,
                    max_tokens: None,
                    prices: PriceRates {
                        input: RateSpec::Fixed(8.0),
                        ..Default::default()
                    },
                }],
                schedules: vec![PriceSchedule {
                    label: Some("peak".into()),
                    timezone: Some("UTC".into()),
                    periods: vec![SchedulePeriod {
                        start_time: "12:00".into(),
                        end_time: "14:00".into(),
                        weekdays: Some(vec!["mon".into(), "fri".into()]),
                        prices: PriceRates {
                            input: RateSpec::Fixed(30.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            },
            tier: TIER_EXTERNAL,
        });
        // 普通条目：无分段无时间规则。
        p.add_entry(Entry {
            prefix: match_key("plain-model"),
            display: "plain-model".into(),
            name: None,
            plan: PricePlan {
                base: PriceRates {
                    input: RateSpec::Fixed(1.0),
                    output: RateSpec::Fixed(2.0),
                    cache_write: RateSpec::Fixed(0.25),
                    cache_read: RateSpec::Fixed(0.02),
                },
                ..Default::default()
            },
            tier: TIER_MODELSDEV,
        });
        let entries = p.entries();
        let t = entries
            .iter()
            .find(|e| e.prefix == "nano-gpt/qwen/tiered-view")
            .unwrap();
        assert_eq!(t.source, "外置");
        assert_eq!(t.channel.as_deref(), Some("nano-gpt"));
        assert_eq!(t.name.as_deref(), Some("Tiered View"));
        assert_eq!(t.basis, Some(PricingBasis::PromptTokens));
        assert!(t.has_tiered_pricing);
        assert_eq!(t.segments.len(), 1);
        assert_eq!(
            (t.segments[0].min_tokens, t.segments[0].max_tokens),
            (272_001, None)
        );
        assert_eq!(t.segments[0].label.as_deref(), Some(">272K"));
        assert_eq!(t.segments[0].prices.input, Some(8.0));
        assert_eq!(t.schedules.len(), 1);
        let period = &t.schedules[0].periods[0];
        assert_eq!(
            (period.start_time.as_str(), period.end_time.as_str()),
            ("12:00", "14:00")
        );
        assert_eq!(
            period.weekdays.as_ref().map(|w| w.len()),
            Some(2),
            "星期限制随规则透出"
        );
        // 基础价视图：显式 0 保留、缺失 = None（不折叠成 0）。
        assert_eq!(t.cache_write, Some(0.0));
        assert_eq!(t.cache_read, None);
        assert!(t.incomplete);
        // 普通条目：保持现有布局语义（无分段/时间规则、四价齐全）。
        let pl = entries.iter().find(|e| e.prefix == "plain-model").unwrap();
        assert!(!pl.has_tiered_pricing);
        assert!(pl.segments.is_empty() && pl.schedules.is_empty());
        assert_eq!(pl.channel, None);
        assert_eq!((pl.input, pl.cache_read), (Some(1.0), Some(0.02)));
        // 序列化：segments/schedules 可见，未知分项为 null。
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"min_tokens\":272001"));
        assert!(json.contains("\"cache_read\":null"));
        assert!(json.contains("\"cache_write\":0.0"));
        let json_pl = serde_json::to_string(&pl).unwrap();
        assert!(!json_pl.contains("segments"), "无分段条目跳过分段字段");
    }

    #[test]
    fn test_pricing_cost_breakdown() {
        // Task 5：统一估算器输出请求级 breakdown——四行明细与总价同源；
        // 阈值边界、部分缺价、峰谷与分段条件切换、重复估算确定性。
        let rates = |i: f64, o: f64, cw: f64, cr: f64| PriceRates {
            input: RateSpec::Fixed(i),
            output: RateSpec::Fixed(o),
            cache_write: RateSpec::Fixed(cw),
            cache_read: RateSpec::Fixed(cr),
        };
        let mut p = Pricing::empty();
        // 渠道 fixed：固定四价，无分段无时间规则。
        p.add_entry(Entry {
            prefix: match_key("chan-fixed/bd"),
            display: "chan-fixed/bd".into(),
            name: None,
            plan: PricePlan {
                base: rates(8.0, 4.0, 1.0, 0.5),
                ..Default::default()
            },
            tier: TIER_OPENROUTER,
        });
        // 渠道 tiered：基础 2/1/0.5/缺 + >272001 段（只覆盖 input/output，
        // cache 回退基础，cache_read 全程缺价）。
        p.add_entry(Entry {
            prefix: match_key("chan-tiered/bd"),
            display: "chan-tiered/bd".into(),
            name: None,
            plan: PricePlan {
                basis: Some(PricingBasis::PromptTokens),
                base: PriceRates {
                    input: RateSpec::Fixed(2.0),
                    output: RateSpec::Fixed(1.0),
                    cache_write: RateSpec::Fixed(0.5),
                    cache_read: RateSpec::Unknown,
                },
                segments: vec![PriceSegment {
                    label: Some(">272K".into()),
                    min_tokens: 272_001,
                    max_tokens: None,
                    prices: PriceRates {
                        input: RateSpec::Fixed(10.0),
                        output: RateSpec::Fixed(5.0),
                        cache_write: RateSpec::Unknown,
                        cache_read: RateSpec::Unknown,
                    },
                }],
                ..Default::default()
            },
            tier: TIER_OPENROUTER,
        });
        // 渠道 peak：基础 1/2/缺/缺 + 工作日 12:00-14:00 UTC 峰时 input 30。
        p.add_entry(Entry {
            prefix: match_key("chan-peak/bd"),
            display: "chan-peak/bd".into(),
            name: None,
            plan: PricePlan {
                base: PriceRates {
                    input: RateSpec::Fixed(1.0),
                    output: RateSpec::Fixed(2.0),
                    cache_write: RateSpec::Unknown,
                    cache_read: RateSpec::Unknown,
                },
                schedules: vec![PriceSchedule {
                    label: Some("peak".into()),
                    timezone: Some("UTC".into()),
                    periods: vec![SchedulePeriod {
                        start_time: "12:00".into(),
                        end_time: "14:00".into(),
                        weekdays: Some(
                            ["mon", "tue", "wed", "thu", "fri"]
                                .iter()
                                .map(|s| s.to_string())
                                .collect(),
                        ),
                        prices: PriceRates {
                            input: RateSpec::Fixed(30.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
            tier: TIER_OPENROUTER,
        });
        fn line_of(est: &CostEstimate, kind: CostLineKind) -> &CostLine {
            est.lines.iter().find(|l| l.kind == kind).unwrap()
        }
        let monday = "2026-01-05"; // 周一
        let noon: jiff::Timestamp = format!("{monday}T13:00:00Z").parse().unwrap();
        let afternoon: jiff::Timestamp = format!("{monday}T15:00:00Z").parse().unwrap();

        // 场景 1：峰时 + 低于阈值。候选总价 fixed=1.02（完整）、
        // tiered=0.255（cache_read 缺）、peak=3.1（cache 两项缺）→ peak 胜出。
        let tc = counts(100_000, 50_000, 10_000, 20_000);
        let est = p.estimate("chan-fixed/bd", &tc, noon).unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-peak/bd");
        assert!((est.cost - 3.1).abs() < 1e-9);
        assert!(!est.complete, "峰时候选缺缓存价 → 部分计价");
        assert_eq!(est.unknown.cache_write, 10_000);
        assert_eq!(est.unknown.cache_read, 20_000);
        assert_eq!(est.basis_value, 130_000, "basis = input+cw+cr");
        assert_eq!(est.basis, Some(PricingBasis::PromptTokens));
        assert_eq!(
            est.matched.as_ref().unwrap().schedule_label.as_deref(),
            Some("peak")
        );
        assert_eq!(est.segment_label, None);
        // 四行明细：input 峰价 30、output 规则回退基础 2、缓存两项未计价。
        let li = line_of(&est, CostLineKind::Input);
        assert_eq!((li.tokens, li.unit_price), (100_000, Some(30.0)));
        assert!((li.subtotal - 3.0).abs() < 1e-9);
        let lo = line_of(&est, CostLineKind::Output);
        assert_eq!((lo.tokens, lo.unit_price), (50_000, Some(2.0)));
        assert!((lo.subtotal - 0.1).abs() < 1e-9);
        for kind in [CostLineKind::CacheWrite, CostLineKind::CacheRead] {
            let l = line_of(&est, kind);
            assert!(!l.priced && l.unit_price.is_none() && l.subtotal == 0.0);
        }
        assert_eq!(est.lines.len(), 4);

        // 场景 2：非峰时同请求 → fixed 1.02 完整计价胜出。
        let est = p.estimate("chan-fixed/bd", &tc, afternoon).unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-fixed/bd");
        assert!(est.complete);
        assert_eq!(est.unknown, TokenCounts::default());
        let li = line_of(&est, CostLineKind::Input);
        assert_eq!((li.tokens, li.unit_price), (100_000, Some(8.0)));
        let lcr = line_of(&est, CostLineKind::CacheRead);
        assert_eq!(
            (lcr.tokens, lcr.unit_price, lcr.priced),
            (20_000, Some(0.5), true)
        );
        assert!((lcr.subtotal - 20_000.0 * 0.5 / 1e6).abs() < 1e-12);

        // 场景 3：恰好超过阈值 272001 → tiered 高档（input 10/output 5，
        // cache_write 回退 0.5，cache_read 缺价）胜过 fixed 的高价。
        let tc_hi = counts(272_001, 1_000, 0, 0);
        let est = p.estimate("chan-fixed/bd", &tc_hi, afternoon).unwrap();
        // fixed = 272001*8 + 1000*4 = 2.180008；tiered = 272001*10 + 1000*5 = 2.72501。
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-tiered/bd");
        assert_eq!(est.segment_label.as_deref(), Some(">272K"));
        assert!((est.cost - 2.72501).abs() < 1e-9);
        assert_eq!(est.basis_value, 272_001);
        let li = line_of(&est, CostLineKind::Input);
        assert_eq!(li.unit_price, Some(10.0));
        // 低于阈值 272000 → tiered 基础价 2 便宜不过 fixed 8 → fixed 胜。
        let tc_edge = counts(272_000, 1_000, 0, 0);
        let est = p.estimate("chan-fixed/bd", &tc_edge, afternoon).unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-fixed/bd");
        assert_eq!(est.segment_label, None);

        // 场景 4：分段只覆盖部分价格字段——tiered 命中高档时 cache_write
        // 回退基础 0.5 而非 0/未知。
        let tc_cw = counts(272_001, 0, 1_000, 0);
        let est = p.estimate("chan-tiered/bd", &tc_cw, afternoon).unwrap();
        let l = line_of(&est, CostLineKind::CacheWrite);
        assert_eq!(
            (l.unit_price, l.priced),
            (Some(0.5), true),
            "分段缺 cache → 回退基础价"
        );

        // 场景 5：完全缺价候选（无基础无分段命中）——所有 token 进 unknown。
        let mut p2 = Pricing::empty();
        p2.add_entry(Entry {
            prefix: match_key("chan-none/bd"),
            display: "chan-none/bd".into(),
            name: None,
            plan: PricePlan::default(),
            tier: TIER_OPENROUTER,
        });
        let est = p2
            .estimate("chan-none/bd", &counts(1_000, 2_000, 0, 0), afternoon)
            .unwrap();
        assert_eq!(est.cost, 0.0);
        assert!(!est.complete);
        assert_eq!(est.unknown.input, 1_000);
        assert_eq!(est.unknown.output, 2_000);
        assert!(est.lines.iter().all(|l| !l.priced));

        // 场景 6：同一请求估算两次 → breakdown 与总价完全相同（纯函数）。
        let a = p.estimate("chan-fixed/bd", &tc, noon).unwrap();
        let b = p.estimate("chan-fixed/bd", &tc, noon).unwrap();
        assert_eq!(a, b);
        // breakdown 行可序列化（Task 6/7 的 DTO 载体）。
        let json = serde_json::to_string(&a.lines).unwrap();
        assert!(json.contains("\"kind\":\"input\""));
        assert!(json.contains("\"priced\":true"));
    }

    #[test]
    fn test_pricing_time_condition_candidate() {
        // Task 4A：峰谷时间规则参与候选最高费用裁决；历史事件按事件时间
        // 与规则时区换算，绝不读当前墙上时钟。
        let mut p = Pricing::empty();
        // 渠道 A：无时间规则，固定 input 8.0。
        p.add_entry(Entry {
            prefix: match_key("chan-a/timed"),
            display: "chan-a/timed".into(),
            name: None,
            plan: PricePlan {
                base: PriceRates {
                    input: RateSpec::Fixed(8.0),
                    ..Default::default()
                },
                ..Default::default()
            },
            tier: TIER_OPENROUTER,
        });
        // 渠道 B：UTC 峰时窗 08:00-20:00（工作日）input 20.0；规则级基线 2.0。
        p.add_entry(Entry {
            prefix: match_key("chan-b/timed"),
            display: "chan-b/timed".into(),
            name: None,
            plan: PricePlan {
                base: PriceRates::default(),
                schedules: vec![PriceSchedule {
                    label: Some("peak".into()),
                    timezone: Some("UTC".into()),
                    periods: vec![SchedulePeriod {
                        start_time: "08:00".into(),
                        end_time: "20:00".into(),
                        weekdays: Some(
                            ["mon", "tue", "wed", "thu", "fri"]
                                .iter()
                                .map(|s| s.to_string())
                                .collect(),
                        ),
                        prices: PriceRates {
                            input: RateSpec::Fixed(20.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }],
                    prices: PriceRates {
                        input: RateSpec::Fixed(2.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            },
            tier: TIER_OPENROUTER,
        });
        let counts1m = counts(1_000_000, 0, 0, 0);
        // 2026-01-05 是周一：12:00 UTC 在峰时窗内 → B 20.0 > A 8.0 → B。
        let monday_noon: jiff::Timestamp = "2026-01-05T12:00:00Z".parse().unwrap();
        let est = p.estimate("chan-a/timed", &counts1m, monday_noon).unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-b/timed");
        assert!((est.cost - 20.0).abs() < 1e-9);
        let m = est.matched.as_ref().unwrap();
        assert_eq!(m.schedule_label.as_deref(), Some("peak"));
        assert_eq!(m.schedule_timezone.as_deref(), Some("UTC"));
        assert_eq!(m.request_at.as_deref(), Some("2026-01-05T12:00:00Z"));
        // 同日 23:00 UTC（非高峰）→ B 规则级基线 2.0 < A 8.0 → A。
        let monday_night: jiff::Timestamp = "2026-01-05T23:00:00Z".parse().unwrap();
        let est = p.estimate("chan-a/timed", &counts1m, monday_night).unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-a/timed");
        assert!((est.cost - 8.0).abs() < 1e-9);
        // 周六（2026-01-10）12:00 UTC：峰时窗有星期限制 → 不命中 → B 2.0 → A。
        let saturday: jiff::Timestamp = "2026-01-10T12:00:00Z".parse().unwrap();
        let est = p.estimate("chan-a/timed", &counts1m, saturday).unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-a/timed");

        // 声明时区生效：Asia/Shanghai（UTC+8）本地峰时窗 = UTC 00:00-12:00。
        let mut p2 = Pricing::empty();
        for (raw, sched) in [
            (
                "chan-c/tz",
                Some(PriceSchedule {
                    label: Some("peak".into()),
                    timezone: Some("Asia/Shanghai".into()),
                    periods: vec![SchedulePeriod {
                        start_time: "08:00".into(),
                        end_time: "20:00".into(),
                        weekdays: None,
                        prices: PriceRates {
                            input: RateSpec::Fixed(30.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }],
                    prices: PriceRates {
                        input: RateSpec::Fixed(1.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            ),
            ("chan-d/tz", None),
        ] {
            p2.add_entry(Entry {
                prefix: match_key(raw),
                display: raw.to_string(),
                name: None,
                plan: PricePlan {
                    base: PriceRates {
                        input: RateSpec::Fixed(9.0),
                        ..Default::default()
                    },
                    schedules: sched.into_iter().collect(),
                    ..Default::default()
                },
                tier: TIER_OPENROUTER,
            });
        }
        // 周一 04:00 UTC = 上海 12:00 → 峰时 30.0 > 9.0 → C。
        let est = p2
            .estimate(
                "chan-c/tz",
                &counts1m,
                "2026-01-05T04:00:00Z".parse().unwrap(),
            )
            .unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-c/tz");
        assert!((est.cost - 30.0).abs() < 1e-9);
        // 周一 20:00 UTC = 上海周二 04:00（窗外）→ 规则级 1.0 < 9.0 → D。
        let est = p2
            .estimate(
                "chan-c/tz",
                &counts1m,
                "2026-01-05T20:00:00Z".parse().unwrap(),
            )
            .unwrap();
        assert_eq!(est.matched.as_ref().unwrap().raw_key, "chan-d/tz");
        assert!((est.cost - 9.0).abs() < 1e-9);

        // 同渠道重叠窗口：按当前条件取更贵的完整规则（不逐项拼价）。
        let mut p3 = Pricing::empty();
        p3.add_entry(Entry {
            prefix: match_key("chan-e/ov"),
            display: "chan-e/ov".into(),
            name: None,
            plan: PricePlan {
                base: PriceRates::default(),
                schedules: vec![PriceSchedule {
                    label: Some("double".into()),
                    timezone: Some("UTC".into()),
                    periods: vec![
                        SchedulePeriod {
                            start_time: "08:00".into(),
                            end_time: "20:00".into(),
                            weekdays: None,
                            prices: PriceRates {
                                input: RateSpec::Fixed(5.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        SchedulePeriod {
                            start_time: "10:00".into(),
                            end_time: "18:00".into(),
                            weekdays: None,
                            prices: PriceRates {
                                input: RateSpec::Fixed(12.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    ],
                    prices: PriceRates::default(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            tier: TIER_OPENROUTER,
        });
        // 12:00 两窗同时命中 → 取更贵规则 12.0。
        let est = p3.estimate("chan-e/ov", &counts1m, monday_noon).unwrap();
        assert!((est.cost - 12.0).abs() < 1e-9);
        assert_eq!(
            est.matched.as_ref().unwrap().schedule_label.as_deref(),
            Some("double")
        );
        // 09:00 只命中第一窗 → 5.0。
        let est = p3
            .estimate(
                "chan-e/ov",
                &counts1m,
                "2026-01-05T09:00:00Z".parse().unwrap(),
            )
            .unwrap();
        assert!((est.cost - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_pricing_external_segment() {
        // Task 4：外置 TOML 分段语法。合法条目带分段生效；非法 basis、负价、
        // 重叠区间、空洞（未封尾）与未支持的时间规则各自给出诊断且不影响
        // 同文件其他条目。
        let dir = std::env::temp_dir().join(format!("tokenscope-ext-seg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = write(
            &dir,
            "pricing.toml",
            r#"
[[model]]
prefix = "gpt-5.6"
basis = "prompt_tokens"
application = "whole_request"
input = 4.0
output = 20.0
cache_write = 5.0
cache_read = 0.4

[[model.segment]]
label = ">272K"
min_tokens = 272001
input = 8.0
output = 30.0
cache_write = 10.0
cache_read = 0.8

[[model]]
prefix = "bad-basis"
basis = "audio_seconds"
input = 1.0
output = 1.0
cache_write = 0.0
cache_read = 0.0

[[model]]
prefix = "neg-price"
input = -1.0
output = 1.0
cache_write = 0.0
cache_read = 0.0

[[model]]
prefix = "overlap-seg"
input = 1.0
output = 1.0
cache_write = 0.0
cache_read = 0.0

[[model.segment]]
min_tokens = 100000
max_tokens = 200000
input = 2.0
output = 2.0
cache_write = 0.0
cache_read = 0.0

[[model.segment]]
min_tokens = 150000
input = 3.0
output = 3.0
cache_write = 0.0
cache_read = 0.0

[[model]]
prefix = "gap-seg"
input = 1.0
output = 1.0
cache_write = 0.0
cache_read = 0.0

[[model.segment]]
min_tokens = 100000
max_tokens = 200000
input = 2.0
output = 2.0
cache_write = 0.0
cache_read = 0.0

[[model.segment]]
min_tokens = 300000
input = 3.0
output = 3.0
cache_write = 0.0
cache_read = 0.0

[[model]]
prefix = "with-schedule"
input = 1.0
output = 1.0
cache_write = 0.0
cache_read = 0.0

[[model.schedule]]
timezone = "Mars/Olympus"

[[model.schedule.period]]
start_time = "00:00"
end_time = "08:00"
input = 0.5
output = 0.5
cache_write = 0.0
cache_read = 0.0
"#,
        );
        let (p, warnings) = Pricing::load(Some(&path), None, None);
        assert_eq!(warnings.len(), 5, "warnings: {warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("bad-basis")));
        assert!(warnings.iter().any(|w| w.contains("neg-price")));
        assert!(warnings.iter().any(|w| w.contains("overlap-seg")));
        assert!(warnings.iter().any(|w| w.contains("gap-seg")));
        assert!(warnings.iter().any(|w| w.contains("with-schedule")));
        // 非法条目不得静默生效；时间规则非法只跳过规则，条目仍按基础价生效。
        assert!(p.lookup("bad-basis").is_none());
        assert!(p.lookup("neg-price").is_none());
        assert!(p.lookup("overlap-seg").is_none());
        assert!(p.lookup("gap-seg").is_none());
        let est = p
            .estimate("with-schedule", &counts(1_000, 0, 0, 0), at())
            .unwrap();
        assert!(est.complete, "时间规则被跳过 → 按基础价完整计价");
        assert!((est.cost - 1_000.0 * 1.0 / 1e6).abs() < 1e-12);
        // 合法条目带分段生效：272000 基础档、272001 高档。
        let est = p
            .estimate("gpt-5.6", &counts(272_000, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 272_000.0 * 4.0 / 1e6).abs() < 1e-9);
        let est = p
            .estimate("gpt-5.6", &counts(272_001, 0, 0, 0), at())
            .unwrap();
        assert!((est.cost - 272_001.0 * 8.0 / 1e6).abs() < 1e-9);
        let m = est.matched.as_ref().unwrap();
        assert_eq!(m.source, "external");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_external_missing_is_silent() {
        let (p, warnings) = Pricing::load(Some(Path::new("Z:/no-such/pricing.toml")), None, None);
        assert_eq!(p.external_count(), 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_pricing_external_override_and_append() {
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-ext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = write(
            &dir,
            "pricing.toml",
            r#"
[[model]]
prefix = "claude-opus-5"
input = 9.0
output = 9.0
cache_write = 9.0
cache_read = 9.0

[[model]]
prefix = "my-model/zen"
input = 1.0
output = 2.0
cache_write = 0.0
cache_read = 0.0
"#,
        );
        let (p, warnings) = Pricing::load(Some(&path), None, None);
        assert!(warnings.is_empty());
        assert_eq!(p.external_count(), 2);
        assert_eq!(
            p.lookup("claude-opus-5").unwrap().plan.base.input,
            Some(9.0)
        );
        assert_eq!(
            p.lookup("my-model/zen-2").unwrap().plan.base.output,
            Some(2.0)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_external_broken_no_fallback() {
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-extbad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = write(&dir, "pricing.toml", "not valid toml [[[");
        let (p, warnings) = Pricing::load(Some(&path), None, None);
        assert_eq!(p.external_count(), 0);
        assert!(
            p.lookup("claude-opus-5").is_none(),
            "坏外置后无任何兜底来源 → 未知"
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("解析失败"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_tier_order() {
        // 三层候选并存（外置 > OpenRouter > models.dev）。lookup 按
        // tie-break（来源优先级）取层级高者；estimate 按本次请求费用取
        // 最高者——此处外置 42 > openrouter 3，两种口径都选外置。
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-tier-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let external = write(
            &dir,
            "pricing.toml",
            // 外置与 openrouter 同前缀（claude-sonnet-4-5）：层级优先，外置 42 胜出
            "[[model]]\nprefix = \"claude-sonnet-4-5\"\ninput = 42.0\noutput = 42.0\ncache_write = 0.0\ncache_read = 0.0\n",
        );
        let snapshot = write(
            &dir,
            "pricing-openrouter.json",
            r#"{"synced_at":"t","entries":[
                {"id":"anthropic/claude-sonnet-4.5","name":"Claude Sonnet 4.5",
                 "prompt":0.000003,"completion":0.000015,
                 "cache_read":0.0000003,"cache_write":0.00000375},
                {"id":"openai/gpt-5.2","name":"GPT-5.2",
                 "prompt":0.000007,"completion":0.00005,"cache_read":0,"cache_write":0},
                {"id":"tencent/hy3:free","name":"HY3 free",
                 "prompt":0,"completion":0,"cache_read":0,"cache_write":0}
            ]}"#,
        );
        let (p, warnings) = Pricing::load(Some(&external), None, Some(&snapshot));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");

        // 外置层压过 openrouter（同前缀，层级优先）
        let hit = p.lookup("claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(hit.plan.base.input, Some(42.0));
        // openrouter 层生效：点号命名归一化命中（无内置兜底，独立计价）
        assert_eq!(
            p.lookup("gpt-5.2-20260101").unwrap().plan.base.input,
            Some(7.0)
        );
        // Task 1：外置/openrouter 都没有的模型 → unknown（无编译期兜底）
        assert!(p.lookup("claude-sonnet-4").is_none());
        // 免费变体经最长前缀命中 :free 条目 → 0 价（known，非 unknown）
        let free = p.lookup("tencent/hy3:free").unwrap();
        assert_eq!(free.plan.base.input, Some(0.0));
        // 条目来源标识与显示名
        let entries = p.entries();

        assert!(
            entries
                .iter()
                .any(|e| e.source == "OpenRouter" && e.name.as_deref() == Some("Claude Sonnet 4.5"))
        );
        // 悬浮对照：外置行的同前缀 openrouter 对照价仍然挂接
        let sonnet_ext = entries
            .iter()
            .find(|e| e.source == "外置" && e.prefix == "claude-sonnet-4-5")
            .expect("外置 sonnet-4-5 行应存在");
        let or = sonnet_ext
            .openrouter
            .as_ref()
            .expect("同前缀 openrouter 条目应挂上对照价");
        assert!((or.input - 3.0).abs() < 1e-9);
        assert_eq!(or.name.as_deref(), Some("Claude Sonnet 4.5"));
        // Task 1：不再有"内置"来源行
        assert!(entries.iter().all(|e| e.source != "内置"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_variant_isolation() {
        // Task 2A：变体条目只在 variant 完全一致时精确命中；查询 variant
        // 无同变体条目时回退到基名价格，但匹配方式必须显式标记 fallback。
        // `:free` 条目永远不得在回退段命中其他变体（回退只对无 variant 条目）。
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-var-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snapshot = write(
            &dir,
            "pricing-openrouter.json",
            r#"{"synced_at":"t","entries":[
                {"id":"tencent/hy3","name":"HY3",
                 "prompt":0.0000000825,"completion":0.0000004,"cache_read":0,"cache_write":0},
                {"id":"tencent/hy3:free","name":"HY3 free",
                 "prompt":0,"completion":0,"cache_read":0,"cache_write":0}
            ]}"#,
        );
        let (p, warnings) = Pricing::load(None, None, Some(&snapshot));
        assert!(warnings.is_empty());
        // 精确变体命中 0 价
        assert_eq!(
            p.lookup("tencent/hy3:free").unwrap().plan.base.input,
            Some(0.0)
        );
        // 基名命中基名价格
        assert!(
            (p.lookup("tencent/hy3")
                .unwrap()
                .plan
                .base
                .input
                .resolve_direct()
                .unwrap()
                - 0.0825)
                .abs()
                < 1e-9
        );
        // 未知变体：回退到基名价格（variant fallback 显式标记，非静默）
        let est = p
            .estimate("tencent/hy3:preview", &counts(1_000_000, 0, 0, 0), at())
            .unwrap();
        assert!(
            (est.cost - 0.0825).abs() < 1e-9,
            "无同变体条目 → 按基名价格估算"
        );
        let matched = est.matched.expect("命中候选必须有元数据");
        assert_eq!(matched.match_mode, MatchMode::FullVariantFallback);
        assert_eq!(matched.raw_key, "tencent/hy3");
        // 只有 :free 条目时，其他变体不吃免费价（回退段仅接受无 variant 条目）
        let dir2 = std::env::temp_dir().join(format!("tokenscope-m5-var2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir2);
        let snapshot2 = write(
            &dir2,
            "pricing-openrouter.json",
            r#"{"synced_at":"t","entries":[
                {"id":"tencent/hy3:free","name":"HY3 free",
                 "prompt":0,"completion":0,"cache_read":0,"cache_write":0}
            ]}"#,
        );
        let (p2, _) = Pricing::load(None, None, Some(&snapshot2));
        assert!(
            p2.estimate("tencent/hy3:preview", &counts(1, 0, 0, 0), at())
                .is_none()
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&dir2).ok();
    }

    // ── 缓存读取定价解析（RateSpec 三态）回归 ──────────────────

    /// 外置单条目 helper：base 四价（Option 语义经 from_options）。
    fn external_entry(toml_body: &str) -> (Pricing, Vec<String>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("tokenscope-ratespec-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = write(&dir, "pricing.toml", toml_body);
        let (p, warnings) = Pricing::load(Some(&path), None, None);
        std::fs::remove_dir_all(&dir).ok();
        (p, warnings)
    }

    #[test]
    fn test_ratespec_same_as_input_base_resolves_to_input_price() {
        // 计划不变量 3/4：显式 same_as_input 沿用输入价；显式 0 仍是免费；
        // Unknown 仍未知。三者互不混淆。
        let (p, w) = external_entry(
            r#"
[[model]]
prefix = "m/same-as-input"
input = 2.0
output = 10.0
cache_write = 0.0
cache_read = "same_as_input"

[[model]]
prefix = "m/free-read"
input = 2.0
output = 10.0
cache_read = 0.0

[[model]]
prefix = "m/unknown-read"
input = 2.0
output = 10.0
"#,
        );
        assert!(w.is_empty(), "{w:?}");
        let t = counts(1_000, 0, 0, 50_000);
        // same_as_input：cache_read 按输入价 2.0 计费 → 50_000 × 2 / 1e6 = 0.1
        let est = p.estimate("same-as-input", &t, at()).unwrap();
        assert!(est.complete, "same_as_input 解析后必须完整");
        assert_eq!(est.unknown.cache_read, 0);
        let cr = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::CacheRead)
            .unwrap();
        assert_eq!(cr.unit_price, Some(2.0));
        assert_eq!(cr.rate_kind, RateKind::SameAsInput);
        assert!((est.cost - (1_000.0 * 2.0 + 50_000.0 * 2.0) / 1e6).abs() < 1e-9);
        // 显式 0：免费（known），不是未知
        let est = p.estimate("free-read", &t, at()).unwrap();
        assert!(est.complete);
        let cr = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::CacheRead)
            .unwrap();
        assert_eq!(cr.unit_price, Some(0.0));
        assert_eq!(cr.rate_kind, RateKind::Fixed);
        assert_eq!(cr.subtotal, 0.0);
        // Unknown：缺价 ≠ 免费
        let est = p.estimate("unknown-read", &t, at()).unwrap();
        assert!(!est.complete);
        assert_eq!(est.unknown.cache_read, 50_000);
        let cr = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::CacheRead)
            .unwrap();
        assert_eq!(cr.rate_kind, RateKind::Unknown);
        assert_eq!(cr.unit_price, None);
    }

    #[test]
    fn test_ratespec_same_as_input_follows_segment_and_schedule_input() {
        // 计划不变量 5：分段/峰谷档的 same_as_input 引用同层解析出的输入价。
        let (p, w) = external_entry(
            r#"
[[model]]
prefix = "tiered/sai"
input = 2.0
output = 10.0
cache_read = "same_as_input"

[[model.segment]]
label = ">272K"
min_tokens = 272001
input = 8.0
output = 30.0
cache_read = "same_as_input"

[[model]]
prefix = "sched/peak-sai"
input = 2.0
output = 10.0
cache_read = "same_as_input"

[[model.schedule]]
label = "peak"
timezone = "UTC"
[[model.schedule.period]]
start_time = "00:00"
end_time = "23:59"
input = 30.0
"#,
        );
        assert!(w.is_empty(), "{w:?}");
        let high = counts(0, 0, 0, 300_000);
        // 分段命中：分段 input=8 → cache_read 也是 8（SameAsInput 跟随同层输入价）
        let est = p.estimate("sai", &high, at()).unwrap();
        let cr = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::CacheRead)
            .unwrap();
        assert_eq!(cr.unit_price, Some(8.0));
        assert_eq!(cr.rate_kind, RateKind::SameAsInput);
        assert_eq!(est.segment_label.as_deref(), Some(">272K"));
        // 峰谷命中（无分段模型）：规则级 input=30 → cache_read 也是 30
        let est = p.estimate("peak-sai", &high, at()).unwrap();
        assert_eq!(
            est.matched.as_ref().unwrap().schedule_label.as_deref(),
            Some("peak")
        );
        let cr = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::CacheRead)
            .unwrap();
        assert_eq!(
            cr.unit_price,
            Some(30.0),
            "峰谷档内 same_as_input 跟随该档输入价"
        );
        // 低上下文走基础价 2.0
        let est = p.estimate("sai", &counts(0, 0, 0, 1_000), at()).unwrap();
        let cr = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::CacheRead)
            .unwrap();
        assert_eq!(cr.unit_price, Some(2.0));
    }

    #[test]
    fn test_ratespec_same_as_input_rejected_for_other_components() {
        // 计划不变量：SameAsInput 仅允许 cache_read，其他分项加载即拒绝。
        let (p, w) = external_entry(
            r#"
[[model]]
prefix = "bad/sai-input"
input = "same_as_input"
output = 10.0
"#,
        );
        assert_eq!(p.external_count(), 0, "非法声明必须整条拒绝");
        assert!(w.iter().any(|x| x.contains("same_as_input")), "{w:?}");
    }

    #[test]
    fn test_ratespec_missing_cache_read_with_zero_tokens_stays_complete() {
        // 计划 Task 1 Step 1：cache_read 缺价但请求 cache_read token 为 0
        // → 不因该字段标为不完整。
        let (p, w) = external_entry(
            r#"
[[model]]
prefix = "zero-cr/m"
input = 2.0
output = 10.0
"#,
        );
        assert!(w.is_empty());
        let est = p.estimate("m", &counts(1_000, 500, 0, 0), at()).unwrap();
        assert!(est.complete);
        assert_eq!(est.unknown.cache_read, 0);
    }

    #[test]
    fn test_ratespec_serde_wire_format() {
        // 线格式：Fixed → 数字（含 0）、SameAsInput → "same_as_input"、
        // Unknown → null；旧 v4 JSON（数字/null）可按 v5 语义解析。
        assert_eq!(
            serde_json::to_value(PriceRates {
                input: RateSpec::Fixed(0.0),
                output: RateSpec::Unknown,
                cache_write: RateSpec::SameAsInput,
                cache_read: RateSpec::Fixed(1.5),
            })
            .unwrap(),
            serde_json::json!({"input": 0.0, "output": null, "cache_write": "same_as_input", "cache_read": 1.5})
        );
        let r: PriceRates = serde_json::from_str(r#"{"input": 1.5, "cache_read": null}"#).unwrap();
        assert_eq!(r.input, RateSpec::Fixed(1.5));
        assert_eq!(r.cache_read, RateSpec::Unknown);
        // 缺省字段 = Unknown（旧索引缺键兼容）
        let r: PriceRates = serde_json::from_str("{}").unwrap();
        assert_eq!(r, PriceRates::default());
    }

    #[test]
    fn test_index_v4_json_still_loads_as_v5_semantics() {
        // 计划不变量 10：旧索引可读——数字 → Fixed、null/缺失 → Unknown，
        // 结果语义不变；索引版本接受 v ≤ 5。
        let v4 = r#"{
            "v": 4, "sig": "s", "synced_at": "t", "warnings": [],
            "entries": [{
                "prefix": "m", "display": "vendor/m", "name": null, "tier": 1,
                "plan": {"base": {"input": 2.0, "output": null}},
                "input": 2.0, "output": null, "cache_write": null, "cache_read": null
            }]
        }"#;
        let index: PricingIndex = serde_json::from_str(v4).unwrap();
        let p = Pricing::from_index(&index);
        let plan = p.lookup("m").unwrap().plan;
        assert_eq!(plan.base.input, RateSpec::Fixed(2.0));
        assert_eq!(plan.base.output, RateSpec::Unknown);
        assert_eq!(plan.base.cache_read, RateSpec::Unknown);
    }

    #[test]
    fn test_model_policy_applies_same_as_input_without_overriding_explicit() {
        // 计划 Task 3：model_policy 只改写候选缺失的 cache_read 声明；
        // 已有明确数字/显式 0 的候选不得被覆盖；不生成新候选。
        let dir = std::env::temp_dir().join(format!("tokenscope-policy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = write(
            &dir,
            "pricing-modelsdev.json",
            r#"{"v":3,"synced_at":"t","entries":[
                {"id":"zenmux/gpt-5.4","name":null,"input":4.0,"output":20.0},
                {"id":"cortecs/gpt-5.4","name":"C","input":3.0,"output":18.0,"cache_read":0.3},
                {"id":"openai/gpt-5.4","name":null,"input":2.5,"output":15.0,"cache_read":0.0}
            ]}"#,
        );
        let toml = write(
            &dir,
            "pricing.toml",
            r#"
[[model_policy]]
prefix = "gpt-5.4"
cache_read = "same_as_input"
"#,
        );
        let (p, warnings) = Pricing::load(Some(&toml), Some(&snap), None);
        assert!(warnings.is_empty(), "{warnings:?}");
        // zenmux（缺 cache_read）被策略声明为沿用输入价；本请求下它是最高
        // 费用候选 → 主估算完整，cache_read 单价 = 该候选输入价 4.0。
        let est = p
            .estimate("gpt-5.4", &counts(1_000, 0, 0, 100_000), at())
            .unwrap();
        assert!(est.complete, "策略应用后 zenmux 必须完整可计价");
        let cr = est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::CacheRead)
            .unwrap();
        assert_eq!(cr.unit_price, Some(4.0));
        assert_eq!(cr.rate_kind, RateKind::SameAsInput);
        // cortecs 的显式 0.3 与 openai 的显式 0（免费）未被覆盖
        let entries = p.entries();
        let cortecs = entries
            .iter()
            .find(|e| e.channel.as_deref() == Some("cortecs"))
            .unwrap();
        assert_eq!(cortecs.cache_read, Some(0.3));
        let openai = entries
            .iter()
            .find(|e| e.channel.as_deref() == Some("openai"))
            .unwrap();
        assert_eq!(openai.cache_read, Some(0.0), "显式 0 不得被策略覆盖");
        // 策略不生成新候选（外置条目数为 0）
        assert_eq!(p.external_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_model_policy_channel_filter_and_miss_warning() {
        let dir = std::env::temp_dir().join(format!("tokenscope-policy2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let snap = write(
            &dir,
            "pricing-modelsdev.json",
            r#"{"v":3,"synced_at":"t","entries":[
                {"id":"zenmux/gpt-5.4","name":null,"input":4.0,"output":20.0}
            ]}"#,
        );
        let toml = write(
            &dir,
            "pricing.toml",
            r#"
[[model_policy]]
prefix = "gpt-5.4"
channel = "nope"
cache_read = "same_as_input"
"#,
        );
        let (p, warnings) = Pricing::load(Some(&toml), Some(&snap), None);
        // 限定过严 → 明确告警，不静默制造价格
        assert!(
            warnings
                .iter()
                .any(|x| x.contains("model_policy") && x.contains("没有命中")),
            "{warnings:?}"
        );
        let est = p
            .estimate("gpt-5.4", &counts(1_000, 0, 0, 100_000), at())
            .unwrap();
        assert!(!est.complete, "未命中策略 → cache_read 仍未知");
        assert_eq!(est.unknown.cache_read, 100_000);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_model_policy_invalid_value_warns() {
        let dir = std::env::temp_dir().join(format!("tokenscope-policy3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let toml = write(
            &dir,
            "pricing.toml",
            r#"
[[model_policy]]
prefix = "some-model"
cache_read = "free"
"#,
        );
        let (p, warnings) = Pricing::load(Some(&toml), None, None);
        assert_eq!(p.external_count(), 0);
        assert!(
            warnings
                .iter()
                .any(|x| x.contains("不支持的 cache_read 值")),
            "{warnings:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_corrupt_snapshot_warns() {
        let dir =
            std::env::temp_dir().join(format!("tokenscope-m5-snapbad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snapshot = write(&dir, "pricing-openrouter.json", "not json");
        let (p, warnings) = Pricing::load(None, None, Some(&snapshot));
        assert_eq!(p.openrouter_count(), 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("OpenRouter"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
