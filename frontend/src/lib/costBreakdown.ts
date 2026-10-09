// Task 6（设计系统）：费用明细 tooltip 的展示模型——只做展示层排版，
// 不做任何费用重算（不变量 7：金额一律来自后端 breakdown）。
// 结构按 DESIGN.md §5「事实 → 公式 → 结果 → 来源」排列。
import { fmtNum } from "../types";
import { formatMoney } from "./formatMoney";
import { TOKEN_BUCKETS, UNIT_PRICE_SUFFIX_AFTER_AMOUNT, tokenBucketLabel } from "./tokenDisplay";
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

const SOURCE_LABEL: Record<string, string> = {
  external: "外置价格表",
  "models.dev": "models.dev",
  openrouter: "OpenRouter",
};

const MATCH_LABEL: Record<MatchMode, string> = {
  full: "完整匹配",
  full_variant_fallback: "完整匹配（变体回退）",
  prefix: "前缀匹配",
  prefix_variant_fallback: "前缀匹配（变体回退）",
};

// UX07：分项显示名消费 tokenDisplay 单一来源（U14：缓存命中）。
export const KIND_LABEL: Record<CostLineKind, string> = Object.fromEntries(
  TOKEN_BUCKETS.map((b) => [b.key, b.key === "input" ? "输入（扣除缓存）" : b.label]),
) as Record<CostLineKind, string>;

/** USD/百万 token 单价（UX07：unit 场景单一入口）。 */
function fmtUnit(v: number): string {
  return formatMoney(v, "unit");
}

/** 金额展示：仅排版舍入，不参与计算；极小非零金额不得显示为 $0.00。 */
/** 金额展示（UX07：request 场景单一入口，极小非零不得显示为 $0.00）。 */
function fmtMoney(v: number): string {
  return formatMoney(v, "request");
}

function factRows(bd: EventCostBreakdown): BreakdownRow[] {
  const m = bd.matched;
  const input = bd.lines.find((l) => l.kind === "input")?.tokens ?? 0;
  const cw = bd.lines.find((l) => l.kind === "cache_write")?.tokens ?? 0;
  const cr = bd.lines.find((l) => l.kind === "cache_read")?.tokens ?? 0;
  const rows: BreakdownRow[] = [
    { label: "匹配模型", value: m.raw_key },
    { label: "渠道", value: m.channel ?? "暂无数据" },
    { label: "请求时间", value: m.request_at ?? "暂无数据" },
    {
      label: "prompt tokens",
      detail: `输入（扣除缓存） ${fmtNum(input)} + ${tokenBucketLabel("cache_write")} ${fmtNum(cw)} + ${tokenBucketLabel("cache_read")} ${fmtNum(cr)} = 总输入 ${fmtNum(bd.basis_value)}`,
    },
    { label: "命中档位", value: bd.segment_label ?? "基础价档" },
    {
      label: "时间档",
      value: m.schedule_label
        ? `${m.schedule_label}（时区 ${m.schedule_timezone ?? "暂无数据"}）`
        : "无峰谷规则",
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
        label: KIND_LABEL[l.kind],
        value: `${fmtNum(l.tokens)} token，金额超出可表示范围`,
        unknown: true,
      };
    }
    if (!l.priced || l.unit_price == null) {
      return {
        label: KIND_LABEL[l.kind],
        value: `${fmtNum(l.tokens)} token，缺少单价`,
        unknown: l.tokens > 0,
      };
    }
    // same_as_input：单价来自同层输入价（unit_price 已由后端解析为实际数值），
    // 与普通固定价区分显示（缓存读取定价解析计划 Task 5）。
    const unit =
      l.rate_kind === "same_as_input" ? `输入价 ${fmtUnit(l.unit_price)}` : fmtUnit(l.unit_price);
    return {
      label: KIND_LABEL[l.kind],
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
      label: "结果",
      value: "无法估算（全部分项缺少单价）",
      unknown: true,
      total: true,
    });
  } else {
    rows.push({ label: "估算合计", value: fmtMoney(bd.cost_usd), total: true });
  }
  if (!bd.complete) {
    rows.push({
      label: "未计价",
      detail: "缺价 ≠ 免费：未计价 token 不计入合计，数量见上方各行",
      unknown: true,
    });
  }
  return rows;
}

function sourceRows(bd: EventCostBreakdown): BreakdownRow[] {
  const m = bd.matched;
  const selection =
    m.reason === "highest_partial_cost"
      ? `候选 ${m.candidate_count} 条（均不完整），取已知费用最高者；未计价 token 见未知行`
      : `候选 ${m.candidate_count} 条（完整 ${m.complete_candidate_count ?? "?"}/不完整 ${
          m.incomplete_candidate_count ?? "?"
        }），在完整候选中取最高费用（保守估算，非服务器实际路由）`;
  const rows: BreakdownRow[] = [
    { label: "计价来源", value: SOURCE_LABEL[m.source] ?? m.source },
    { label: "匹配方式", value: MATCH_LABEL[m.match_mode] ?? m.match_mode },
    { label: "候选选择", detail: selection },
  ];
  if (bd.excluded_candidate_warning) {
    rows.push({ label: "估算范围", detail: bd.excluded_candidate_warning, unknown: true });
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
      if (r.detail != null) return [r.label, r.detail].filter(Boolean).join("：");
      return [r.label, r.value].filter(Boolean).join(" ");
    })
    .join("\n");
}
