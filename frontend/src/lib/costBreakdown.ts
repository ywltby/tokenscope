// Task 6（设计系统）：费用明细 tooltip 的展示模型——只做展示层排版，
// 不做任何费用重算（不变量 7：金额一律来自后端 breakdown）。
// 结构按 DESIGN.md §5「事实 → 公式 → 结果 → 来源」排列。
import { fmtNum } from "../types";
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

const KIND_LABEL: Record<CostLineKind, string> = {
  input: "输入",
  output: "输出",
  cache_write: "缓存写",
  cache_read: "缓存读",
};

/** USD/百万 token 单价（0 显示 $0；小值保 6 位防长尾）。 */
function fmtUnit(v: number): string {
  if (v === 0) return "$0";
  if (v < 0.001) return `$${v.toFixed(6)}`;
  if (v < 1) return `$${v.toFixed(4)}`;
  return `$${v.toFixed(2)}`;
}

/** 金额展示：仅排版舍入，不参与计算；极小非零金额不得显示为 $0.00。 */
function fmtMoney(v: number): string {
  if (v === 0) return "$0.00";
  if (Math.abs(v) < 0.01) return `$${v.toFixed(6)}`;
  return `$${v.toFixed(2)}`;
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
      detail: `输入 ${fmtNum(input)} + 缓存写 ${fmtNum(cw)} + 缓存读 ${fmtNum(cr)} = ${fmtNum(bd.basis_value)}`,
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
    if (!l.priced || l.unit_price == null) {
      return {
        label: KIND_LABEL[l.kind],
        value: `${fmtNum(l.tokens)} token，缺少单价`,
        unknown: l.tokens > 0,
      };
    }
    return {
      label: KIND_LABEL[l.kind],
      value: `${fmtNum(l.tokens)} × ${fmtUnit(l.unit_price)}/M = ${fmtMoney(l.subtotal)}`,
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
  return [
    { label: "计价来源", value: SOURCE_LABEL[m.source] ?? m.source },
    { label: "匹配方式", value: MATCH_LABEL[m.match_mode] ?? m.match_mode },
    {
      label: "候选选择",
      detail: `候选 ${m.candidate_count} 条，按本请求条件取最高费用（保守估算，非服务器实际路由）`,
    },
  ];
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
