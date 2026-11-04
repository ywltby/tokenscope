// Task 6（设计系统）：费用明细 tooltip 的展示模型——只做展示层排版，
// 不做任何费用重算（不变量 7：金额一律来自后端 breakdown）。
// 结构按 DESIGN.md §5「事实 → 公式 → 结果 → 来源」排列。
import { fmtNum } from "../types";
import { formatMoney } from "./formatMoney";
import { TOKEN_BUCKETS, UNIT_PRICE_SUFFIX_AFTER_AMOUNT, tokenBucketLabel } from "./tokenDisplay";
import { t } from "./translate";
import type { CostLine, CostLineKind, EventCostBreakdown, MatchMode } from "../types";

/** 单行展示模型：label/value 两列；detail 为整行说明；divider 分组。 */
export interface BreakdownRow {
  label: string;
  value?: string;
  detail?: string;
  /** 缺价/无法估算行——由调用方用警告色渲染 */
  unknown?: boolean;
  /** 分组分隔线 */
  divider?: boolean;
  /** 结果行加粗 */
  total?: boolean;
}

/** 计价来源显示名（`models.dev` / `OpenRouter` 是品牌名，不翻译）。 */
function sourceLabel(source: string): string {
  return source === "external" ? t("cost.sourceExternal") : source;
}

const MATCH_LABEL_KEY: Record<MatchMode, string> = {
  full: "cost.matchFull",
  full_variant_fallback: "cost.matchFullVariant",
  prefix: "cost.matchPrefix",
  prefix_variant_fallback: "cost.matchPrefixVariant",
};

// UX07：分项显示名消费 tokenDisplay 单一来源（U14：缓存命中）；
// N18：5m/1h 分档行不在四桶清单里，标签在此补齐。
// i18n：改为**函数**（读取时才解析），调用点都在渲染期，语言切换后自然更新。
export function kindLabel(kind: CostLineKind): string {
  if (kind === "input") return t("buckets.inputNet");
  if (kind === "cache_write_5m") return t("buckets.cacheWrite5m");
  if (kind === "cache_write_1h") return t("buckets.cacheWrite1h");
  return tokenBucketLabel(kind as (typeof TOKEN_BUCKETS)[number]["key"]);
}

/** USD/百万 token 单价（UX07：unit 场景单一入口）。 */
function fmtUnit(v: number): string {
  return formatMoney(v, "unit");
}

/** 金额展示：仅排版舍入，不参与计算；极小非零金额不得显示为 $0.00。 */
/** 金额展示（UX07：request 场景单一入口，极小非零不得显示为 $0.00）。 */
function fmtMoney(v: number): string {
  return formatMoney(v, "request");
}

/** N18：prompt token 算式里的「缓存写」分量。
 *  可信分档事件把写入拆成 5m/1h 两行（无合并行）——算式必须显示**两档
 *  之和**；只有合并行时才用合并值。两行都缺（候选未声明分档、写入整体
 *  未知）时回落合并值；分档行与合并行都不存在则为 0。绝不重复计数。 */
function cacheWriteTokens(bd: EventCostBreakdown): number {
  const split = bd.lines.filter((l) => l.kind === "cache_write_5m" || l.kind === "cache_write_1h");
  if (split.length > 0) return split.reduce((sum, l) => sum + l.tokens, 0);
  return bd.lines.find((l) => l.kind === "cache_write")?.tokens ?? 0;
}

function factRows(bd: EventCostBreakdown): BreakdownRow[] {
  const m = bd.matched;
  const input = bd.lines.find((l) => l.kind === "input")?.tokens ?? 0;
  const cw = cacheWriteTokens(bd);
  const cr = bd.lines.find((l) => l.kind === "cache_read")?.tokens ?? 0;
  const rows: BreakdownRow[] = [
    { label: t("cost.matchedModel"), value: m.raw_key },
    { label: t("cost.channel"), value: m.channel ?? t("cost.noData") },
    { label: t("cost.requestAt"), value: m.request_at ?? t("cost.noData") },
    {
      label: t("cost.promptTokens"),
      detail: t("cost.inputFormula", {
        input: fmtNum(input),
        cacheWrite: `${tokenBucketLabel("cache_write")} ${fmtNum(cw)}`,
        cacheRead: `${tokenBucketLabel("cache_read")} ${fmtNum(cr)}`,
        total: fmtNum(bd.basis_value),
      }),
    },
    { label: t("cost.hitSegment"), value: bd.segment_label ?? t("cost.baseSegment") },
    {
      label: t("cost.timeSegment"),
      value: m.schedule_label
        ? `${m.schedule_label}${t("cost.tzSuffix", { tz: m.schedule_timezone ?? t("cost.noData") })}`
        : t("cost.noPeakRule"),
    },
  ];
  return rows;
}

function formulaRows(lines: CostLine[]): BreakdownRow[] {
  return lines.map((l) => {
    if (l.overflow) {
      // F04：单价已知但 token × 单价超出可表示范围——分项不计金额、
      // token 保留为未计价；不得显示为免费或 $0。
      return {
        label: kindLabel(l.kind),
        value: t("cost.overflowValue", { tokens: fmtNum(l.tokens) }),
        unknown: true,
      };
    }
    if (!l.priced || l.unit_price == null) {
      return {
        label: kindLabel(l.kind),
        value: t("cost.missingUnit", { tokens: fmtNum(l.tokens) }),
        unknown: l.tokens > 0,
      };
    }
    // same_as_input：单价来自同层输入价（unit_price 已由后端解析为实际数值），
    // 与普通固定价区分显示（缓存读取定价解析计划 Task 5）。
    const unit =
      l.rate_kind === "same_as_input"
        ? t("cost.inputPrice", { price: fmtUnit(l.unit_price) })
        : fmtUnit(l.unit_price);
    return {
      label: kindLabel(l.kind),
      // RC07：单位来自 tokenDisplay 的单一来源（不再各处自造 "/M" 字面量），
      // 完整量纲在浮层内另有说明（USD / 1M token）。金额仍来自后端 DTO。
      value: `${fmtNum(l.tokens)} × ${unit}${UNIT_PRICE_SUFFIX_AFTER_AMOUNT} = ${fmtMoney(l.subtotal)}`,
    };
  });
}

function resultRows(bd: EventCostBreakdown): BreakdownRow[] {
  const rows: BreakdownRow[] = [];
  const unpriced = bd.lines.filter((l) => !l.priced && l.tokens > 0);
  if (bd.cost_usd === 0 && !bd.complete && unpriced.length > 0) {
    // 完全未知：不得显示 $0.00
    rows.push({
      label: t("cost.result"),
      value: t("cost.cannotEstimate"),
      unknown: true,
      total: true,
    });
  } else {
    rows.push({ label: t("cost.estimateTotal"), value: fmtMoney(bd.cost_usd), total: true });
  }
  if (bd.cache_write_split_estimated) {
    // N18：写入分项未知（旧日志/源日志已删）——金额是按写入总量单价的
    // 估算，不得当作完整精确费用；分档拆分见候选声明。
    rows.push({
      label: t("cost.writeTierUnknown"),
      detail: t("cost.writeTierUnknownDetail"),
      unknown: true,
    });
  }
  // 只有**实际存在未计价 token** 才显示未计价说明：「按合并价估算」虽然
  // complete=false，但 unknown token 为 0、金额来自真实单价，不得声称
  // 含未计价 token（复核项 6）。
  if (unpriced.length > 0) {
    rows.push({
      label: t("cost.unpriced"),
      detail: t("cost.unpricedDetail"),
      unknown: true,
    });
  }
  return rows;
}

function sourceRows(bd: EventCostBreakdown): BreakdownRow[] {
  const m = bd.matched;
  // 复核项 4：reason=highest_partial_cost 也可能是**纯估算**（事件缺分档、
  // 候选分档价齐全，unknown token=0）。此时不得声称有未计价 token。
  const selection =
    m.reason === "highest_partial_cost"
      ? bd.cache_write_split_estimated
        ? // 复核项 3：只描述**所选候选**的已知事实——事件缺写入分档、
          // 写费按合并价估算。其他候选为何不完整由后端的排除披露逐条
          // 说明，这里不得一律归因为事件缺分档。
          t("cost.pickHighestPartial", { count: m.candidate_count })
        : t("cost.pickKnownHighest", { count: m.candidate_count })
      : t("cost.pickHighestComplete", {
          count: m.candidate_count,
          complete: m.complete_candidate_count ?? "?",
          incomplete: m.incomplete_candidate_count ?? "?",
        });
  const rows: BreakdownRow[] = [
    { label: t("cost.priceSource"), value: sourceLabel(m.source) },
    { label: t("cost.matchMode"), value: t(MATCH_LABEL_KEY[m.match_mode]) },
    { label: t("cost.candidatePick"), detail: selection },
  ];
  if (bd.excluded_candidate_warning) {
    rows.push({
      label: t("cost.estimateScope"),
      detail: bd.excluded_candidate_warning,
      unknown: true,
    });
  }
  return rows;
}

/** 事实 → 公式 → 结果 → 来源，组间 divider。 */
export function formatCostBreakdownRows(bd: EventCostBreakdown): BreakdownRow[] {
  return [
    ...factRows(bd),
    { label: "", divider: true },
    ...formulaRows(bd.lines),
    { label: "", divider: true },
    ...resultRows(bd),
    { label: "", divider: true },
    ...sourceRows(bd),
  ];
}

/** 纯文本形式（可访问名称/单测对照用）。 */
export function formatCostBreakdownText(bd: EventCostBreakdown): string {
  return formatCostBreakdownRows(bd)
    .map((r) => {
      if (r.divider) return "──────────";
      if (r.detail != null) return [r.label, r.detail].filter(Boolean).join(t("cost.labelJoin"));
      return [r.label, r.value].filter(Boolean).join(" ");
    })
    .join("\n");
}
