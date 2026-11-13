//! 聚合层：把事件按日（Asia/Shanghai 自然日）/ 模型 / 项目分组并计价。
//! 层间契约：只依赖 model 与 pricing，不触碰任何 source 内部类型。

use std::collections::BTreeMap;

use jiff::tz::TimeZone;
use serde::Serialize;

use crate::model::{AgentKind, TokenCounts, UsageEvent};
use crate::model_identity::ModelIdentity;
use crate::pricing::Pricing;

/// MP04：聚合中间态——分组累计 + 该组原始模型名的稳定代表写法。
#[derive(Default)]
struct GroupAccum {
    group: Group,
    representative: Option<String>,
}

/// MP04：Model 维度的展示名——优先可信 models.dev 模型名（按完整等价 ID
/// 命中且与条目顺序无关），其次该组原始模型名的稳定代表，绝不展示压缩后的
/// 身份键（`claudeopus55` 这类写法只用于筛选与下钻匹配）。
fn model_label(key: &str, representative: Option<&str>, pricing: &Pricing) -> String {
    let rep = representative.unwrap_or(key);
    pricing
        .display_name_for(rep)
        .or_else(|| pricing.display_name_for(key))
        .unwrap_or_else(|| rep.to_string())
}

/// 时区解析（M6）：显式传入 > 本机系统（`--tz local`）> 默认 Asia/Shanghai。
/// 存储层永远持有 UTC（见 cache.rs），全链路只在此解析一次。
pub fn resolve_tz(explicit: Option<&str>) -> anyhow::Result<(TimeZone, String)> {
    let label = |tz: &TimeZone, fallback: &str| -> String {
        tz.iana_name().unwrap_or(fallback).to_string()
    };
    match explicit {
        Some(name) if name.eq_ignore_ascii_case("local") => {
            let tz = TimeZone::system();
            Ok((tz.clone(), label(&tz, "local")))
        }
        Some(name) => {
            let tz = TimeZone::get(name).map_err(|e| anyhow::anyhow!("未知时区 {name:?}: {e}"))?;
            Ok((tz, name.to_string()))
        }
        None => {
            let tz = TimeZone::get("Asia/Shanghai").expect("仓库默认时区常量且合法");
            Ok((tz, "Asia/Shanghai".to_string()))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBy {
    #[default]
    Day,
    Model,
    Project,
    Agent,
}

impl GroupBy {
    pub fn as_str(self) -> &'static str {
        match self {
            GroupBy::Day => "day",
            GroupBy::Model => "model",
            GroupBy::Project => "project",
            GroupBy::Agent => "agent",
        }
    }

    /// 表格首列标题。
    pub fn label(self) -> &'static str {
        match self {
            GroupBy::Day => "日期",
            GroupBy::Model => "模型",
            GroupBy::Project => "项目",
            GroupBy::Agent => "应用",
        }
    }
}

/// 一个分组的累计：requests / token 四类 / 已计价费用 / 无价格模型的 token（不变量 5：
/// 未收录模型的用量单独可见，费用只含已计价部分，不得按 0 静默吞掉）。
/// `agents` 仅在多 agent 数据时填充（单 agent 报告保持 M1 输出不变）。
#[derive(Debug, Default, Clone, Serialize)]
pub struct Group {
    pub key: String,
    /// C2：展示名（Project 维度 = 路径末段；其他维度 None，展示用 key）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub requests: u64,
    pub tokens: TokenCounts,
    pub cost_usd: f64,
    pub unknown_pricing: bool,
    /// N18：本组含「事件缺写入分档、按合并价估算」的请求——独立于
    /// `unknown_pricing`（后者只表示真有未计价 token）。UI 必须分开披露。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cache_write_split_estimated: bool,
    /// T3（2026-11-13）：本组含「线上来源缺缓存价、按 0 估算」的请求——
    /// 金额已按 0 计入、没有未计价 token，但不是完整官方价格（独立披露）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cache_price_zero_estimated: bool,
    pub unknown_tokens: TokenCounts,
    /// H06：本组未知费用的**原因**（如 `rollup_only` = 该组含只有日粒度的历史，
    /// 不能按当前档位/峰谷重估）。缺价（模型未收录）时为 None，两者的处置同为
    /// "计入未知 token"，但原因不同，界面必须区分。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<&'static str>,
    /// T2（2026-11-13）：本组含**来源选择最终采用**的日汇总时，这些汇总行
    /// 携带的 CCS 来源记录费用之和（受检累计；`None` = 组内没有任何带来源
    /// 费用的汇总贡献）。这是来源记录值，不是 TokenScope 请求级估算，
    /// 也不是实际账单——前端必须单独披露，不得与主估算 `cost_usd` 混同。
    /// 缺失（来源未提供费用）时如实按无来源费用处理，不按 0 冒充。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<&'static str>,
}

/// H06：日汇总贡献——只有日粒度的历史（CCS 归并后的快照）。
///
/// 不伪造成请求事件：没有逐请求时间、会话与 cwd，也没有 prompt 规模，无法按
/// 当前上下文档位与峰谷规则重估费用。聚合时它的四桶计入「未知 token」，原因
/// 标注为「历史数据只有汇总」；来源费用只作为来源值保存，不进入主估算。
#[derive(Debug, Clone, PartialEq)]
pub struct RollupContribution {
    /// 来源统计日（来源时区的自然日）。
    pub day: String,
    /// 来源统计时区标识（用于时区不匹配判定）。
    pub source_tz: String,
    pub app: AgentKind,
    /// 原始模型名（分组时按等价身份归一）。
    pub model: String,
    pub tokens: TokenCounts,
    pub requests: u64,
    /// 来源费用（CCS 记录值，仅作来源证据）。
    pub source_cost_usd: Option<f64>,
    /// 逻辑来源标识。
    pub source: String,
}

/// 桶覆盖诊断（进入报告，供界面解释"为什么这部分费用是未知的"）。
///
/// 字段保持**蛇形**命名：与 `SummaryReport`/`EventRow` 等既有 IPC 载荷一致，
/// 前端类型逐字段对应（早期误加 camelCase 改名会让前端读到 undefined，
/// 时区提示因此整段消失）。
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct RollupCoverage {
    /// 按**请求明细**统计的桶数（A 类 + C 类）。
    pub detail_buckets: u64,
    /// 改用日汇总的桶数（桶内没有明细）。
    pub rollup_buckets: u64,
    /// B 类：快照内明细逐项小于汇总、已按日汇总补足被清理历史的桶数
    /// （明细仍可查看，但不重复计入总量；快照之后新增的请求独立计入）。
    #[serde(default)]
    pub covered_buckets: u64,
    /// 上述被汇总替代的明细 token（已计入统计的是汇总值；这些只是明细侧的
    /// 观察量，用于向用户解释"明细看到了多少、为什么总量更大"）。
    #[serde(default)]
    pub covered_detail_tokens: TokenCounts,
    /// C 类：请求数与四桶方向**交叉**，既不能用明细逐项覆盖汇总、也不能用
    /// 汇总吸收明细的桶数。这些桶按请求明细统计、汇总不叠加，不做逐字段取
    /// 最大或捏造差额；诊断走本结构的独立字段，不进入 `SummaryReport.warnings`
    /// （采集警告计数）。字段沿用旧名以兼容既有 IPC 契约与回归断言。
    pub unresolved_buckets: u64,
    /// 上述交叉冲突桶中汇总侧的四桶合计（仅作对账规模，不表示"丢失了这么多
    /// token"——两侧不是可以相加的同量数据）。
    pub unresolved_covered_tokens: TokenCounts,
    /// A 类：快照范围内明细逐项覆盖汇总（含完全相等），已按请求明细统计、
    /// 汇总不叠加的桶数。这是**统计来源选择**，不声称两侧请求集合已被证明
    /// 完全相同；正常路径不产生告警，依据由界面放在可展开的统计说明里。
    #[serde(default)]
    pub detail_selected_buckets: u64,
    /// C 类交叉冲突的明确类型（稳定键，去重后排序）：`crossed_token_buckets`
    /// = 四桶方向有交叉；`opposed_requests_tokens` = 请求数与用量方向相反。
    /// 供界面独立显示，不混入采集警告计数。
    #[serde(default)]
    pub conflict_kinds: Vec<String>,
    /// 查询时区与来源日时区不一致时的提示（不为 None 时，日汇总只在不按日
    /// 筛选/分组的查询里计入；按日视图需要切回来源时区或只看请求明细）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone_mismatch: Option<String>,
    /// U01：时刻范围（滚动 24h / 自定义到分钟）不选取任何日汇总桶——所涉
    /// 自然日**确有**日汇总时为 true，界面据此提示"当前范围只统计请求明细"。
    /// 只在确有日汇总时为 true：没有日粒度历史就不提示。界面文案由前端写。
    #[serde(default)]
    pub rollups_excluded_for_instant_range: bool,
    /// 2026-11-05（第三轮）：同桶汇总来自不同时刻导入的多批快照时的披露。
    ///
    /// 覆盖边界只能取**最早**的来源快照时刻（晚于它的明细不可能全在最早那份
    /// 快照里）；落在两批之间的明细已按明细计入，可能与较晚批次的汇总重复。
    /// 数据上无法区分，只能如实披露（`None` = 没有这种桶）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_batch_boundary: Option<String>,
    /// 2026-11-05（第四轮）：日汇总**没有可核对的来源快照时刻**时的披露——
    /// 旧版批次（`import_revision = 1`，批次时间是读取结束后生成的），或该行
    /// 没有导入批次关联（v1 迁移 / 早期导入遗留）。
    ///
    /// 这类批次的时间不能代表来源快照建立时刻，也无法事后判定；统计上按整桶
    /// 信任口径处理（不拿旧时间做边界、不猜造历史），并如实说明可能少算。
    /// 2026-11-07：重导内容完全相同的快照**不**为该行补写批次——重读同一数值
    /// 不能证明快照在本次读取时建立，披露必须保留。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_boundary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Aggregated {
    pub by: &'static str,
    pub groups: Vec<Group>,
    pub totals: Group,
}

/// SF08：聚合算术全部受检——单事件更新先在临时值上全部成功再提交；
/// 多个各自合法的事件累计溢出/金额非有限时返回明确错误（不回绕、
/// 不饱和、不跳过"最后一个"制造不确定统计）。
///
/// MP04：Model 维度的分组键是**模型等价身份**（`mode_identity` 规则），
/// `claude-opus-5-5` / `claude-opus-5.5` 等写法合并为同一组；`Group.label`
/// 优先取可信 models.dev 模型名，否则退回该组原始模型名的稳定代表写法
///（字典序最小，与事件顺序无关）。下钻仍按同一身份规则匹配。
pub fn aggregate(
    events: &[UsageEvent],
    by: GroupBy,
    tz: &TimeZone,
    pricing: &Pricing,
) -> anyhow::Result<Aggregated> {
    aggregate_with_rollups(events, &[], by, tz, pricing)
}

/// 汇总聚合（H06 起支持日汇总贡献）。
///
/// `rollups` 是**只有日粒度**的历史贡献（见 [`RollupContribution`]）：它们的
/// 四桶直接计入分组与总计，但整体计入「未知 token」并标注原因，绝不按当前
/// 价格/档位重估，也不与请求明细相加成"剩余请求"。
pub fn aggregate_with_rollups(
    events: &[UsageEvent],
    rollups: &[RollupContribution],
    by: GroupBy,
    tz: &TimeZone,
    pricing: &Pricing,
) -> anyhow::Result<Aggregated> {
    let mut state = AggregateState::new(by, tz.clone());
    for e in events {
        state.push_event(e, pricing)?;
    }
    for r in rollups {
        state.push_rollup(r)?;
    }
    state.finish(pricing)
}

/// B 类桶（日汇总补足明细）：`来源时区 → {(日, 应用, 等价模型) → 覆盖起点}`。
///
/// 只含 `report::select_rollup_contributions` 判为 B 的桶；A/C 类不在此列，
/// 因此它们的明细一条都不会被跳过。`Some(since)` 表示事件时间不晚于来源快照
/// 建立时刻的明细视为已含于汇总；`None` 表示该桶没有可核对的快照时刻
/// （旧库迁移遗留），整桶明细都按"可能已含于汇总"处理。跳过按时间边界，
/// 不看 `ccs_request` 来源标记。
type CoveredDetailMap = std::collections::HashMap<
    String,
    std::collections::HashMap<(String, String, String), Option<jiff::Timestamp>>,
>;

/// 增量聚合状态（H07）：事件与日汇总贡献**逐条喂入**，内存只随**分组数**
/// 增长（不随事件数）——冻结查询因此可以流式汇总整份历史，不必先把所有事件
/// 物化进内存。
pub struct AggregateState {
    by: GroupBy,
    tz: TimeZone,
    map: BTreeMap<String, GroupAccum>,
    /// 出现过的来源应用（只有多应用数据才在分组里标注 agents）。
    agent_seen: Vec<&'static str>,
    /// 2026-11-05：已被日汇总替代的明细桶。命中且不晚于覆盖起点的事件已计入
    /// 汇总，聚合时跳过以避免重复；起点之后的请求照常计入。
    covered: Option<CoveredDetailMap>,
}

impl AggregateState {
    pub fn new(by: GroupBy, tz: TimeZone) -> Self {
        Self {
            by,
            tz,
            map: BTreeMap::new(),
            agent_seen: Vec::new(),
            covered: None,
        }
    }

    /// 标记"明细已由日汇总替代"的桶（查询入口按覆盖判定结果一次性设置）。
    ///
    /// 这些桶里快照范围内的明细已经在汇总值中，不能重复累计；明细视图仍照常
    /// 展示这些请求，只是不进入总量。
    pub fn set_covered_detail(&mut self, buckets: &[crate::report::CoveredBucket]) {
        if buckets.is_empty() {
            return;
        }
        let mut map: CoveredDetailMap = std::collections::HashMap::new();
        for b in buckets {
            map.entry(b.source_tz.clone()).or_default().insert(
                (
                    b.day.clone(),
                    b.app.as_str().to_string(),
                    b.model_identity.clone(),
                ),
                b.since,
            );
        }
        self.covered = Some(map);
    }

    /// 该事件是否已计入日汇总（同桶、且不晚于覆盖起点；起点缺失视为整桶覆盖）。
    fn covered_by_rollup(&self, e: &EventView<'_>) -> bool {
        let Some(map) = &self.covered else {
            return false;
        };
        let identity = e.model_identity.to_string();
        for (tz_name, buckets) in map {
            let Ok(tz) = TimeZone::get(tz_name) else {
                continue;
            };
            let day = e.ts.to_zoned(tz).date().to_string();
            if let Some(since) = buckets.get(&(day, e.agent.as_str().to_string(), identity.clone()))
            {
                return since.is_none_or(|at| e.ts <= at);
            }
        }
        false
    }

    /// 喂入一条明细事件：落在已覆盖桶且不晚于覆盖起点的事件已计入日汇总，
    /// 跳过以避免重复（明细视图不受影响）；起点之后的请求照常计入。
    pub fn push_event(&mut self, e: &UsageEvent, pricing: &Pricing) -> anyhow::Result<()> {
        let view = EventView::from_usage(e);
        self.push_view(&view, pricing)
    }

    /// 喂入库内事件（查询汇总热路径）：直接借用库字段，不构造带多份
    /// `String` 的 `UsageEvent`；聚合与计价口径与 [`Self::push_event`] 相同。
    pub(crate) fn push_stored_event(
        &mut self,
        e: &crate::history::SummaryEvent,
        pricing: &Pricing,
    ) -> anyhow::Result<()> {
        let view = EventView::from_stored(e);
        self.push_view(&view, pricing)
    }

    fn push_view(&mut self, e: &EventView<'_>, pricing: &Pricing) -> anyhow::Result<()> {
        if self.covered_by_rollup(e) {
            return Ok(());
        }
        if !self.agent_seen.contains(&e.agent.as_str()) {
            self.agent_seen.push(e.agent.as_str());
        }
        push_event_into(&mut self.map, self.by, &self.tz, e, pricing)
    }

    /// 喂入一条日汇总贡献。
    pub fn push_rollup(&mut self, r: &RollupContribution) -> anyhow::Result<()> {
        if !self.agent_seen.contains(&r.app.as_str()) {
            self.agent_seen.push(r.app.as_str());
        }
        push_rollup_into(&mut self.map, self.by, r)
    }
}

/// 聚合所需的最小事件视图：借用查询热路径中的库字段，避免逐事件克隆
/// 模型/项目/会话字符串；计价仍收到完整的四桶与分项。
struct EventView<'a> {
    ts: jiff::Timestamp,
    agent: AgentKind,
    model: &'a str,
    model_identity: std::borrow::Cow<'a, str>,
    project: &'a str,
    tokens: crate::model::TokenCounts,
}

impl<'a> EventView<'a> {
    fn from_usage(e: &'a UsageEvent) -> Self {
        Self {
            ts: e.ts,
            agent: e.agent,
            model: &e.model,
            model_identity: std::borrow::Cow::Owned(ModelIdentity::parse(&e.model).identity_key()),
            project: &e.project,
            tokens: crate::model::TokenCounts::from_event(e),
        }
    }

    fn from_stored(e: &'a crate::history::SummaryEvent) -> Self {
        Self {
            ts: e.ts,
            agent: e.app,
            model: &e.model_raw,
            model_identity: std::borrow::Cow::Borrowed(&e.model_identity),
            project: e.project_key.as_deref().unwrap_or("(未知)"),
            tokens: e.tokens,
        }
    }
}

/// 单条事件的合并逻辑（切片路径与流式路径共用同一实现）。
fn push_event_into(
    map: &mut BTreeMap<String, GroupAccum>,
    by: GroupBy,
    tz: &TimeZone,
    e: &EventView<'_>,
    pricing: &Pricing,
) -> anyhow::Result<()> {
    // 逐条喂入时不知道最终是否多应用：先如实记录，`finish` 再按需清空
    //（与切片路径“单 agent 报告不填 agents”的输出完全一致）。
    const RECORD_AGENTS: bool = true;
    {
        let key = match by {
            GroupBy::Day => e.ts.to_zoned(tz.clone()).date().to_string(),
            GroupBy::Model => e.model_identity.to_string(),
            GroupBy::Project => e.project.to_string(),
            GroupBy::Agent => e.agent.as_str().to_string(),
        };
        let key_err = key.clone();
        let acc = map.entry(key.clone()).or_insert_with(|| GroupAccum {
            group: Group {
                label: None,
                key,
                ..Group::default()
            },
            representative: None,
        });
        // MP04：无可信展示名时的代表写法 = 该组原始模型名的最小字典序（稳定）。
        if by == GroupBy::Model && acc.representative.as_deref().is_none_or(|r| e.model < r) {
            acc.representative = Some(e.model.to_string());
        }
        let g = &mut acc.group;
        // 临时值上全部成功才提交（requests/tokens/unknown/cost）。
        let new_requests = g
            .requests
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("分组 {} 的请求数累计超出可表示范围", key_err))?;
        let event_tokens = e.tokens;
        let mut new_tokens = g.tokens;
        let mut new_unknown = g.unknown_tokens;
        new_tokens = new_tokens
            .checked_add(&event_tokens)
            .ok_or_else(|| anyhow::anyhow!("分组 {} 的 token 累计超出可表示范围", key_err))?;
        let mut new_cost = g.cost_usd;
        // 确定性计价只算一次（百万事件聚合下两次调用是纯浪费）；estimate
        // 不产生失败路径，求值先后不影响"临时值全部成功才提交"的纪律。
        let estimate = pricing.estimate(e.model, &event_tokens, e.ts);
        match &estimate {
            Some(est) => {
                new_cost += est.cost;
                if !new_cost.is_finite() {
                    anyhow::bail!("分组 {} 的费用累计出现非有限值", key_err);
                }
                // B3：部分计价（分项缺价格）不按 0——未知分项的 token 单列，
                // † 标记语义即"费用仅含已计价部分"。
                if !est.complete {
                    new_unknown = new_unknown.checked_add(&est.unknown).ok_or_else(|| {
                        anyhow::anyhow!("分组 {} 的未知 token 累计超出可表示范围", key_err)
                    })?;
                }
            }
            None => {
                new_unknown = new_unknown.checked_add(&event_tokens).ok_or_else(|| {
                    anyhow::anyhow!("分组 {} 的未知 token 累计超出可表示范围", key_err)
                })?;
            }
        }
        // 全部成功 → 提交（agents 追加不涉算术，随后执行）。
        g.requests = new_requests;
        g.tokens = new_tokens;
        g.unknown_tokens = new_unknown;
        g.cost_usd = new_cost;
        // unknown_pricing 是组内累积标记：只有**实际存在未计价 token** 或
        // 无价模型才置位。N18：「按合并价估算」虽然 complete=false，但
        // unknown token 为 0、金额来自真实单价，不得触发「含未计价 token」。
        // 后续完整计价的事件不得清掉已置位的标记。
        g.unknown_pricing = match &estimate {
            Some(est) => g.unknown_pricing || est.unknown.total() > 0,
            None => true,
        };
        g.cache_write_split_estimated |= estimate
            .as_ref()
            .is_some_and(|est| est.cache_write_split_estimated);
        g.cache_price_zero_estimated |= estimate
            .as_ref()
            .is_some_and(|est| est.cache_price_zero_estimated);
        if RECORD_AGENTS && !g.agents.contains(&e.agent.as_str()) {
            g.agents.push(e.agent.as_str());
        }
    }
    Ok(())
}

/// 喂入一条日汇总贡献（H06）：只有日粒度的历史不能按当前价格与档位重估，
/// 也不代表「剩余的独立请求」：四桶整体计入未知 token，原因标注为
/// 「历史数据只有汇总」。
fn push_rollup_into(
    map: &mut BTreeMap<String, GroupAccum>,
    by: GroupBy,
    r: &RollupContribution,
) -> anyhow::Result<()> {
    const RECORD_AGENTS: bool = true;
    {
        let key = match by {
            GroupBy::Day => r.day.clone(),
            GroupBy::Model => ModelIdentity::parse(&r.model).identity_key(),
            // 日汇总没有 cwd/project：与明细侧同一约定，保持「未知」。
            GroupBy::Project => "(未知)".to_string(),
            GroupBy::Agent => r.app.as_str().to_string(),
        };
        let key_err = key.clone();
        let acc = map.entry(key.clone()).or_insert_with(|| GroupAccum {
            group: Group {
                label: None,
                key,
                ..Group::default()
            },
            representative: None,
        });
        if by == GroupBy::Model
            && acc
                .representative
                .as_deref()
                .is_none_or(|rep| r.model.as_str() < rep)
        {
            acc.representative = Some(r.model.clone());
        }
        let g = &mut acc.group;
        g.requests = g
            .requests
            .checked_add(r.requests)
            .ok_or_else(|| anyhow::anyhow!("分组 {key_err} 的请求数累计超出可表示范围"))?;
        g.tokens = g
            .tokens
            .checked_add(&r.tokens)
            .ok_or_else(|| anyhow::anyhow!("分组 {key_err} 的 token 累计超出可表示范围"))?;
        g.unknown_tokens = g
            .unknown_tokens
            .checked_add(&r.tokens)
            .ok_or_else(|| anyhow::anyhow!("分组 {key_err} 的未知 token 累计超出可表示范围"))?;
        g.unknown_pricing = true;
        g.unknown_reason = Some("rollup_only");
        // T2（2026-11-13）：只对来源选择**最终采用**的汇总贡献累计来源记录
        // 费用（本函数只被 rollup_only 桶与 B 类桶调用——A/C 类明细优先，
        // 汇总不进入聚合，因此不会重复计入）。来源费用是来源记录值，与主
        // 估算 `cost_usd` 分开保存；受检累加，非法值（非有限）明确报错。
        if let Some(cost) = r.source_cost_usd {
            if !cost.is_finite() {
                anyhow::bail!("分组 {key_err} 的来源费用出现非有限值");
            }
            let summed = g.source_cost_usd.unwrap_or(0.0) + cost;
            if !summed.is_finite() {
                anyhow::bail!("分组 {key_err} 的来源费用累计出现非有限值");
            }
            g.source_cost_usd = Some(summed);
        }
        if RECORD_AGENTS && !g.agents.contains(&r.app.as_str()) {
            g.agents.push(r.app.as_str());
        }
    }
    Ok(())
}

impl AggregateState {
    /// 收口：生成分组展示名、总计，以及（仅在多应用数据时保留的）agents 标注。
    pub fn finish(self, pricing: &Pricing) -> anyhow::Result<Aggregated> {
        let multi_agent = self.agent_seen.len() > 1;
        let by = self.by;
        let map = self.map;
        // 其他维度的展示名生成器（Project = 路径末段；Day/Agent = None）。
        fn other_label(by: &GroupBy, key: &str) -> Option<String> {
            if *by != GroupBy::Project {
                return None;
            }
            Some(
                key.rsplit(['/', '\\'])
                    .find(|s| !s.is_empty())
                    .unwrap_or(key)
                    .to_string(),
            )
        }
        let mut groups: Vec<Group> = map
            .into_values()
            .map(|acc| {
                let mut g = acc.group;
                g.label = match by {
                    GroupBy::Model => {
                        Some(model_label(&g.key, acc.representative.as_deref(), pricing))
                    }
                    _ => other_label(&by, &g.key),
                };
                g
            })
            .collect();
        // 单应用数据不标注 agents（与 M1 起的输出保持一致）：喂入阶段如实记录，
        // 收口时按最终是否多应用决定保留。
        if !multi_agent {
            for g in &mut groups {
                g.agents.clear();
            }
        }
        let mut totals = Group {
            key: "合计".to_string(),
            ..Group::default()
        };
        for g in &groups {
            totals.requests = totals
                .requests
                .checked_add(g.requests)
                .ok_or_else(|| anyhow::anyhow!("总计请求数累计超出可表示范围"))?;
            totals.tokens = totals
                .tokens
                .checked_add(&g.tokens)
                .ok_or_else(|| anyhow::anyhow!("总计 token 累计超出可表示范围"))?;
            totals.cost_usd += g.cost_usd;
            if !totals.cost_usd.is_finite() {
                anyhow::bail!("总计费用累计出现非有限值");
            }
            // T2（2026-11-13）：来源费用与主估算同构受检累计；任一组携带
            // 来源费用时总计也必须如实呈现（None 组不贡献）。
            if let Some(cost) = g.source_cost_usd {
                let summed = totals.source_cost_usd.unwrap_or(0.0) + cost;
                if !summed.is_finite() {
                    anyhow::bail!("总计来源费用累计出现非有限值");
                }
                totals.source_cost_usd = Some(summed);
            }
            totals.unknown_pricing |= g.unknown_pricing;
            totals.cache_write_split_estimated |= g.cache_write_split_estimated;
            totals.cache_price_zero_estimated |= g.cache_price_zero_estimated;
            if totals.unknown_reason.is_none() {
                totals.unknown_reason = g.unknown_reason;
            }
            totals.unknown_tokens = totals
                .unknown_tokens
                .checked_add(&g.unknown_tokens)
                .ok_or_else(|| anyhow::anyhow!("总计未知 token 累计超出可表示范围"))?;
            for a in &g.agents {
                if !totals.agents.contains(a) {
                    totals.agents.push(a);
                }
            }
        }
        groups.push(totals.clone());
        Ok(Aggregated {
            by: by.as_str(),
            groups,
            totals,
        })
    }
}

/// SF05：预设"近 N 天"的解析结果——统计时区下 `[起始自然日, 今天]` 的
/// 自然日闭区间（口径见 docs/stats-semantics.md §3.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetRange {
    pub from: jiff::civil::Date,
    pub to: jiff::civil::Date,
}

/// 以注入的 as_of（统计时区）解析"近 N 天"：起始自然日到今天的**闭区间**，
/// 不含未来日期。days=0 按既有约定归一为 1；超大 days 使日期减法超界时
/// 返回可读错误，不 panic。纯函数：不读取系统时钟，today 由调用方注入
/// 一次（取代原 filter_days 的内部 now 读取）。
pub fn preset_days_range(as_of: &jiff::Zoned, days: u32) -> anyhow::Result<PresetRange> {
    let days = days.max(1);
    let today = as_of.date();
    // jiff Span 天数合法上限（±7304484 ≈ ±2 万年）；超过必然早于最小可表示
    // 日期，直接返回可读错误——Span::new().days() 对越界值会 panic，不能依赖
    // checked_sub 拦截。
    const MAX_SPAN_DAYS: i64 = 7_304_484;
    let back = i64::from(days) - 1;
    if back > MAX_SPAN_DAYS {
        anyhow::bail!("近 {days} 天的起始日期超出可表示范围");
    }
    let from = today
        .checked_sub(jiff::Span::new().days(back))
        .map_err(|e| anyhow::anyhow!("近 {days} 天的起始日期超出可表示范围（{e}）"))?;
    Ok(PresetRange { from, to: today })
}

/// 只保留预设区间 `[from, to]` 内的事件：统计时区落日界、闭区间、
/// 不含未来日期；每个事件只做一次时区转换（落日语义不变量）。
pub fn filter_preset_days(
    events: Vec<UsageEvent>,
    tz: &TimeZone,
    range: PresetRange,
) -> Vec<UsageEvent> {
    events
        .into_iter()
        .filter(|e| {
            let d = e.ts.to_zoned(tz.clone()).date();
            d >= range.from && d <= range.to
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentKind;

    fn tz() -> TimeZone {
        TimeZone::get("Asia/Shanghai").unwrap()
    }

    fn event(ts: &str, model: &str, input: u64, output: u64) -> UsageEvent {
        UsageEvent {
            ts: ts.parse().unwrap(),
            agent: AgentKind::ClaudeCode,
            model: model.into(),
            session_id: "s".into(),
            record_id: String::new(),
            claude_copy: None,
            codex_response: None,
            codex_legacy_origin: None,
            line: 0,
            source_path: String::new(),
            project: "proj-a".into(),
            session_initial_cwd: None,
            event_cwd: None,
            input_tokens: input,
            output_tokens: output,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
        }
    }

    #[test]
    fn test_daily_boundary() {
        // UTC 15:59:59 = 本地 23:59:59（同日）；UTC 16:00:00 = 本地次日 00:00:00。
        let events = [
            event("2026-07-17T15:59:59.000Z", "m", 1, 1),
            event("2026-07-17T16:00:00.000Z", "m", 10, 10),
        ];
        let agg = aggregate(&events, GroupBy::Day, &tz(), &Pricing::default()).unwrap();
        // 末行为合计
        assert_eq!(agg.groups.len(), 3);
        assert_eq!(agg.groups[0].key, "2026-07-17");
        assert_eq!(agg.groups[0].tokens.input, 1);
        assert_eq!(agg.groups[1].key, "2026-07-18");
        assert_eq!(agg.groups[1].tokens.input, 10);
        assert_eq!(agg.totals.requests, 2);
    }

    #[test]
    fn test_aggregate_by_model_and_project() {
        let events = [
            event("2026-07-17T08:00:00.000Z", "tencent/hy3:free", 1, 1),
            event("2026-07-17T08:01:00.000Z", "claude-sonnet-4-5", 2, 2),
            {
                let mut e = event("2026-07-17T08:02:00.000Z", "claude-sonnet-4-5", 3, 3);
                e.project = "proj-b".into();
                e
            },
        ];
        let agg = aggregate(&events, GroupBy::Model, &tz(), &Pricing::default()).unwrap();
        assert_eq!(agg.groups.len(), 3); // grok、claude、合计
        // MP04：Model 维度的 key 是等价身份键，label 是无可信名称时的原始代表写法。
        assert_eq!(agg.groups[0].key, "claudesonnet45");
        assert_eq!(agg.groups[0].label.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(agg.groups[0].tokens.input, 5);
        // Task 1：默认空价格表 → 所有模型 unknown（无编译期兜底）。
        assert!(agg.groups[0].unknown_pricing);
        assert_eq!(agg.groups[0].unknown_tokens.input, 5);
        assert_eq!(agg.groups[1].key, "tencent/hy3:free");
        assert!(agg.groups[1].unknown_pricing);
        assert_eq!(agg.groups[1].unknown_tokens.input, 1);
        assert_eq!(agg.totals.unknown_tokens.input, 6);

        let agg = aggregate(&events, GroupBy::Project, &tz(), &Pricing::default()).unwrap();
        assert_eq!(agg.groups.len(), 3);
        assert_eq!(agg.groups[0].key, "proj-a");
        assert_eq!(agg.groups[0].tokens.input, 3);
        assert_eq!(agg.groups[1].key, "proj-b");
        assert_eq!(agg.groups[1].tokens.input, 3);
    }

    #[test]
    fn test_aggregate_by_agent() {
        let mut e2 = event("2026-07-17T08:01:00.000Z", "m", 2, 2);
        e2.agent = AgentKind::Codex;
        let events = [event("2026-07-17T08:00:00.000Z", "m", 1, 1), e2];
        let agg = aggregate(&events, GroupBy::Agent, &tz(), &Pricing::default()).unwrap();
        assert_eq!(agg.groups.len(), 3); // claude-code、codex、合计
        assert_eq!(agg.groups[0].key, "claude-code");
        assert_eq!(agg.groups[0].requests, 1);
        assert_eq!(agg.groups[1].key, "codex");
        assert_eq!(agg.groups[1].requests, 1);
        assert_eq!(agg.groups[0].agents, ["claude-code"]);
        assert_eq!(agg.totals.requests, 2);
        assert_eq!(agg.totals.agents, ["claude-code", "codex"]);
    }

    /// SF05：预设近 N 天 = `[起始自然日, 今天]` 闭区间，不含未来日期。
    /// 固定上海"今天"（注入 as_of，不读系统时钟），覆盖：前一日、起点、
    /// 今天 23:59:59.999999、未来一天，以及 UTC 与统计时区跨日。
    #[test]
    fn preset_days_excludes_future_dates() {
        let tz = tz();
        // 上海今天 = 2026-08-10（UTC 当日 16:00 起才算上海 08-11）。
        let as_of = "2026-08-10T12:00:00+08:00[Asia/Shanghai]".parse().unwrap();
        // 近 2 天 → [08-09, 08-10] 闭区间。
        let r = preset_days_range(&as_of, 2).unwrap();
        assert_eq!(r.from.to_string(), "2026-08-09");
        assert_eq!(r.to.to_string(), "2026-08-10");

        let mk = |ts: &str| event(ts, "m", 1, 1);
        let events = vec![
            // 前一日（上海 08-09 10:00 = UTC 02:00）→ 含。
            mk("2026-08-09T02:00:00Z"),
            // 起点当刻（上海 08-09 00:00:00 = UTC 08-08T16:00:00Z）→ 含。
            mk("2026-08-08T16:00:00Z"),
            // 今天 23:59:59.999999（UTC 15:59:59.999999）→ 含。
            mk("2026-08-10T15:59:59.999999999Z"),
            // 未来一天（上海 08-11 00:00:01）→ 排除（旧实现含未来）。
            mk("2026-08-10T16:00:01Z"),
            // 前天（上海 08-08）→ 区间外排除。
            mk("2026-08-08T15:59:59Z"),
        ];
        let kept = filter_preset_days(events, &tz, r);
        assert_eq!(kept.len(), 3, "起点/前日/今天23:59:59 含，未来与区间外排除");

        // days=0 按既有约定归一为 1；days=1 → [今天, 今天]。
        let r1 = preset_days_range(&as_of, 0).unwrap();
        assert_eq!(r1.from, r1.to);
        assert_eq!(r1.to.to_string(), "2026-08-10");

        // 超大 days：日期减法超界返回可读错误，不 panic（旧实现 expect）。
        let err = preset_days_range(&as_of, u32::MAX).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("超出可表示范围"), "可读错误: {msg}");
    }

    /// SF05：一次查询冻结一个 today——as_of 由入口解析一次后贯穿整个
    /// 过滤；跨午夜（as_of 变为 00:00:01）属于下一次查询，不会在同一
    /// 次过滤中出现第二个"今天"。
    #[test]
    fn one_query_uses_one_today_across_midnight() {
        let tz = tz();
        // 查询冻结在 上海今天 23:59:59.999999（午夜前一瞬）。
        let as_of: jiff::Zoned = "2026-08-10T23:59:59.999999999+08:00[Asia/Shanghai]"
            .parse()
            .unwrap();
        let range = preset_days_range(&as_of, 1).unwrap();
        assert_eq!(range.to.to_string(), "2026-08-10", "冻结 as_of 的今天");

        let boundary = vec![
            // 上海 08-10 23:59:59 → 今天内，含。
            event("2026-08-10T15:59:59Z", "m", 1, 1),
            // 上海 08-11 00:00:01 → 冻结查询中的"未来"，排除。
            event("2026-08-10T16:00:01Z", "m", 1, 1),
        ];
        let kept = filter_preset_days(boundary, &tz, range);
        assert_eq!(kept.len(), 1, "同一查询内不允许第二个 today");
        assert_eq!(
            kept[0].ts.to_zoned(tz.clone()).date().to_string(),
            "2026-08-10"
        );

        // 跨日之后的新查询：as_of 解析为 08-11，近 2 天 = [08-10, 08-11]，
        // 原边界事件（08-10 23:59:59 / 08-11 00:00:01）都落在区间内。
        let as_of_next: jiff::Zoned = "2026-08-11T00:00:01+08:00[Asia/Shanghai]".parse().unwrap();
        let range_next = preset_days_range(&as_of_next, 2).unwrap();
        assert_eq!(range_next.from.to_string(), "2026-08-10");
        let events_next = vec![
            event("2026-08-10T15:59:59Z", "m", 1, 1),
            event("2026-08-10T16:00:01Z", "m", 1, 1),
        ];
        assert_eq!(filter_preset_days(events_next, &tz, range_next).len(), 2);
    }
}
