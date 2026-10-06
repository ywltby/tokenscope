// Task 8：设置页分段价格展示的纯函数——只排版，不做任何重算。
// 缺失分项显示"未知"，显式 0 显示 $0；分段与峰谷规则展开为悬浮行。
import { fmtNum } from "../types";
import type { PriceRatesView, PriceScheduleView, PriceSegmentView, PricingEntry } from "../types";

/** 单价展示：null = 未知；0 = $0（显式免费）；小值保 6 位防长尾。 */
export function fmtPriceOrUnknown(v: number | null | undefined): string {
  if (v == null) return "未知";
  if (v === 0) return "$0";
  if (v < 0.001) return `$${v.toFixed(6)}`;
  if (v < 1) return `$${v.toFixed(4)}`;
  return `$${v.toFixed(2)}`;
}

const RATE_LABELS: [keyof PriceRatesView, string][] = [
  ["input", "输入"],
  ["output", "输出"],
  ["cache_write", "缓存写"],
  ["cache_read", "缓存读"],
];

/** 一组四类价格 → "输入 $8.00 · 输出 $30.00 · 缓存写 未知 · 缓存读 未知"。 */
export function formatRates(r: PriceRatesView): string {
  return RATE_LABELS.map(([k, label]) => `${label} ${fmtPriceOrUnknown(r[k])}`).join(" · ");
}

/** 分段范围：">272K：[272,001, ∞)" / "[0, 100,000)"。 */
export function formatSegmentRange(s: PriceSegmentView): string {
  const max = s.max_tokens == null ? "∞" : fmtNum(s.max_tokens);
  return `${s.label ? `${s.label}：` : ""}[${fmtNum(s.min_tokens)}, ${max})`;
}

/** 峰谷规则："peak（UTC）· 12:00–14:00 工作日" / 每天。 */
export function formatSchedule(s: PriceScheduleView): string {
  const tz = s.timezone ?? "UTC";
  const periods = s.periods.length
    ? s.periods
        .map((p) => {
          const days = p.weekdays && p.weekdays.length ? `（${p.weekdays.join(",")}）` : " 每天";
          return `${p.start_time}–${p.end_time}${days}`;
        })
        .join("、")
    : "无具体时段（规则级价格始终适用）";
  return `${s.label ?? "时间规则"}（${tz}）· ${periods}`;
}

/**
 * 模型条目的档位展开（悬浮提示行）：计价依据、上下文分段、峰谷规则。
 * 普通条目（has_tiered_pricing = false）返回空数组，保持现有表格布局。
 */
export function formatTieredPricing(e: PricingEntry): string[] {
  if (!e.has_tiered_pricing) return [];
  const out: string[] = [];
  out.push(`计价依据：${e.basis ?? "prompt_tokens（默认）"}`);
  for (const s of e.segments ?? []) {
    out.push(`分段 ${formatSegmentRange(s)} — ${formatRates(s.prices ?? {})}`);
  }
  for (const sc of e.schedules ?? []) {
    out.push(`峰谷 ${formatSchedule(sc)} — ${formatRates(sc.prices ?? {})}`);
  }
  return out;
}
