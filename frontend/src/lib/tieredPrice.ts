// Task 8：设置页分段价格展示的纯函数——只排版，不做任何重算。
// 缺失分项显示"未知"，显式 0 显示 $0；分段与峰谷规则展开为悬浮行。
import { fmtNum } from "../types";
import { TOKEN_BUCKETS } from "./tokenDisplay";
import { t } from "./translate";
import { formatMoney } from "./formatMoney";
import type {
  PriceRatesView,
  PriceScheduleView,
  PriceSegmentView,
  PricingEntry,
  RateSpecView,
} from "../types";

/** SF07 单价展示（三态）：null = 未知；"same_as_input" = 同输入价
 * （随分段/时间规则解析，主表不冒充任何条件的最终输入价）；
 * 0 = $0（显式免费）；小值保 6 位防长尾。 */
export function fmtPriceOrUnknown(v: RateSpecView | undefined): string {
  if (v == null) return t("dashboard.unknownTag");
  if (v === "same_as_input") return t("cost.sameAsInput");
  // UX07：数值分支接金额单一入口（$0、精度、微小非零保护同族）。
  return formatMoney(v, "unit");
}

// RC07：显示词统一来自 tokenDisplay 单一来源（U14：缓存命中），
// 不再散落「缓存读」；技术键仍是 cache_read。
// i18n：按需读取当前语言（`TOKEN_BUCKETS[].label` 是 getter）。
function rateLabels(): [keyof PriceRatesView, string][] {
  return TOKEN_BUCKETS.map((b) => [b.key, b.label] as [keyof PriceRatesView, string]);
}

/** 一组四类价格 → "输入 $8 · 输出 $30 · 缓存写 未知 · 缓存命中 未知"。 */
export function formatRates(r: PriceRatesView): string {
  return rateLabels()
    .map(([k, label]) => `${label} ${fmtPriceOrUnknown(r[k])}`)
    .join(" · ");
}

/** 分段范围：">272K：[272,001, ∞)" / "[0, 100,000)"。 */
export function formatSegmentRange(s: PriceSegmentView): string {
  const max = s.max_tokens == null ? "∞" : fmtNum(s.max_tokens);
  return `${s.label ? `${s.label}${t("cost.labelJoin")}` : ""}[${fmtNum(s.min_tokens)}, ${max})`;
}

/** 峰谷规则："peak（UTC）· 12:00–14:00 工作日" / 每天。 */
export function formatSchedule(s: PriceScheduleView): string {
  const tz = s.timezone ?? "UTC";
  const periods = s.periods.length
    ? s.periods
        .map((p) => {
          const days =
            p.weekdays && p.weekdays.length
              ? t("tiered.weekdaySuffix", { days: p.weekdays.join(",") })
              : t("tiered.everyDay");
          return `${p.start_time}–${p.end_time}${days}`;
        })
        .join(t("tiered.periodJoin"))
    : t("tiered.noPeriods");
  return t("tiered.scheduleText", {
    label: s.label ?? t("tiered.timeRule"),
    tz,
    periods,
  });
}

/**
 * 模型条目的档位展开（悬浮提示行）：计价依据、上下文分段、峰谷规则。
 * 普通条目（has_tiered_pricing = false）返回空数组，保持现有表格布局。
 */
export function formatTieredPricing(e: PricingEntry): string[] {
  if (!e.has_tiered_pricing) return [];
  const out: string[] = [];
  out.push(t("tiered.basis", { basis: e.basis ?? t("tiered.basisDefault") }));
  for (const s of e.segments ?? []) {
    out.push(
      t("tiered.segment", { range: formatSegmentRange(s), rates: formatRates(s.prices ?? {}) }),
    );
  }
  for (const sc of e.schedules ?? []) {
    out.push(
      t("tiered.schedule", { schedule: formatSchedule(sc), rates: formatRates(sc.prices ?? {}) }),
    );
    for (const period of sc.periods) {
      out.push(
        t("tiered.periodOverride", {
          start: period.start_time,
          end: period.end_time,
          rates: formatRates(period.prices ?? {}),
        }),
      );
    }
  }
  return out;
}
