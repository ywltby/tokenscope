// Task 7：费用悬浮提示的纯函数格式化——只做展示层排版，不做任何费用
// 重算（不变量 7：金额一律来自后端 breakdown，前端不得自行乘加）。
import { fmtNum } from "../types";
import type { CostLine, CostLineKind, EventCostBreakdown, MatchMode } from "../types";

/** 悬浮提示的一行：unknown 行由调用方以醒目颜色渲染。 */
export interface BreakdownLine {
  text: string;
  unknown?: boolean;
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

/** USD/百万 token 单价展示（0 显示 $0；小值保 6 位防长尾）。 */
function fmtUnit(v: number): string {
  if (v === 0) return "$0";
  if (v < 0.001) return `$${v.toFixed(6)}`;
  if (v < 1) return `$${v.toFixed(4)}`;
  return `$${v.toFixed(2)}`;
}

/** 小计/总价展示：后端给定值的短舍入，仅排版不参与计算。 */
function fmtMoney(v: number): string {
  if (v === 0) return "$0.00";
  if (Math.abs(v) < 0.01) return `$${v.toFixed(6)}`;
  return `$${v.toFixed(2)}`;
}

/** 分项明细行：`输入 100,000 × $4.00/M = $0.40`；未计价给可解释文案。 */
function fmtLine(l: CostLine): BreakdownLine {
  if (!l.priced || l.unit_price == null) {
    return {
      text: `${KIND_LABEL[l.kind]} ${fmtNum(l.tokens)}（未计价：缺少单价，不计入总价）`,
      unknown: l.tokens > 0,
    };
  }
  return {
    text: `${KIND_LABEL[l.kind]} ${fmtNum(l.tokens)} × ${fmtUnit(l.unit_price)}/M = ${fmtMoney(l.subtotal)}`,
  };
}

/**
 * 请求级费用明细 → 悬浮提示行列表。
 * 覆盖：来源与模型、原始键/渠道/匹配方式与"候选中最高费用"说明、
 * 请求时间/时间档/时区、prompt 度量式、命中档位、分项明细与总价。
 */
export function formatCostBreakdown(bd: EventCostBreakdown): BreakdownLine[] {
  const out: BreakdownLine[] = [];
  const m = bd.matched;
  const source = SOURCE_LABEL[m.source] ?? m.source;
  out.push({ text: `${m.raw_key}（${source}）` });
  const chan = m.channel ? `渠道 ${m.channel} · ` : "";
  out.push({
    text: `${chan}${MATCH_LABEL[m.match_mode] ?? m.match_mode} · 候选 ${m.candidate_count} 条，按本请求条件取最高费用（保守估算）`,
  });
  out.push({ text: `请求时间 ${m.request_at ?? "未知"}` });
  out.push({
    text: m.schedule_label
      ? `时间档 ${m.schedule_label}（时区 ${m.schedule_timezone ?? "?"}）`
      : "时间档：无峰谷规则",
  });
  const unk = bd.unknown;
  out.push({
    text: `计价依据 prompt tokens = ${fmtNum(bd.lines.find((l) => l.kind === "input")?.tokens ?? 0)} + ${fmtNum(bd.lines.find((l) => l.kind === "cache_write")?.tokens ?? 0)} + ${fmtNum(bd.lines.find((l) => l.kind === "cache_read")?.tokens ?? 0)} = ${fmtNum(bd.basis_value)}`,
  });
  out.push({ text: `命中档位：${bd.segment_label ?? "基础价档"}` });
  for (const l of bd.lines) out.push(fmtLine(l));
  out.push({ text: `合计 ${fmtMoney(bd.cost_usd)}` });
  if (!bd.complete) {
    const parts: string[] = [];
    if (unk.input > 0) parts.push(`输入 ${fmtNum(unk.input)}`);
    if (unk.output > 0) parts.push(`输出 ${fmtNum(unk.output)}`);
    if (unk.cache_write > 0) parts.push(`缓存写 ${fmtNum(unk.cache_write)}`);
    if (unk.cache_read > 0) parts.push(`缓存读 ${fmtNum(unk.cache_read)}`);
    out.push({
      text: `未计价 token（缺价 ≠ 免费）：${parts.join("、") || "无"}`,
      unknown: true,
    });
  }
  return out;
}
