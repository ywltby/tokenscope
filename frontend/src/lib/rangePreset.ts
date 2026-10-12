// U01（UI 调整计划 2026-10-13）：时间预设模型。
//
// 前端**不计算查询边界**：预设只被翻译成 `query_begin` 的参数，边界由后端在
// 查询入口冻结 `as_of` 后一次算出，并随查询句柄/汇总回传（`RangeView`）。
// 按钮标签分两条来源——自然日预设（当日 / 7d…）由 preset 直接可算，自定义
// 的精确形态（自然日区间、时刻区间、至今）取决于后端回传的 range。
import type { RangeView } from "../types";
import { fullDate, shortDate, todayInTz, yearOf } from "./dates";

export type RangePreset =
  | { kind: "all" }
  | { kind: "days"; days: 1 | 7 | 14 | 30 } // 自然日；1 = 当日
  | { kind: "rolling"; hours: 24 } // 过去 24 小时
  | { kind: "custom"; start: string; end: string | "now" }; // 统计时区本地时刻 "YYYY-MM-DDTHH:MM"

/** `query_begin` 的时间参数（四组互斥；全为 null = 不下发时间限制）。 */
export type QueryRangeParams = {
  days: number | null;
  rollingHours: number | null;
  startAt: string | null;
  endAt: string | null;
  untilNow: boolean | null;
};

const DAY_PRESETS: readonly number[] = [1, 7, 14, 30];
const LOCAL_MINUTE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/;

const NO_RANGE: QueryRangeParams = {
  days: null,
  rollingHours: null,
  startAt: null,
  endAt: null,
  untilNow: null,
};

function localMinute(value: unknown): value is string {
  return typeof value === "string" && LOCAL_MINUTE.test(value);
}

/** 预设 → `query_begin` 参数（其余时间参数一律显式 null，避免与旧参数混淆）。 */
export function presetToQueryParams(preset: RangePreset | null): QueryRangeParams {
  if (!preset || preset.kind === "all") return { ...NO_RANGE };
  switch (preset.kind) {
    case "days":
      return { ...NO_RANGE, days: preset.days };
    case "rolling":
      return { ...NO_RANGE, rollingHours: preset.hours };
    case "custom":
      return preset.end === "now"
        ? { ...NO_RANGE, startAt: preset.start, untilNow: true }
        : { ...NO_RANGE, startAt: preset.start, endAt: preset.end };
  }
}

/**
 * 稳定身份字符串（快照比较/筛选身份用）。
 * `null` 与 `{kind:"all"}` 等价——两者都是"全部时间"。
 */
export function presetKey(preset: RangePreset | null): string {
  if (!preset || preset.kind === "all") return "all";
  switch (preset.kind) {
    case "days":
      return `days:${preset.days}`;
    case "rolling":
      return `rolling:${preset.hours}`;
    case "custom":
      return `custom:${preset.start}:${preset.end}`;
  }
}

/** 校验并归一化持久化值；非法/缺失一律回退 `null`（全部时间）。 */
export function normalizePreset(value: unknown): RangePreset | null {
  if (value == null || typeof value !== "object") return null;
  const v = value as Record<string, unknown>;
  if (v.kind === "all") return { kind: "all" };
  if (v.kind === "days" && DAY_PRESETS.includes(Number(v.days))) {
    return { kind: "days", days: Number(v.days) as 1 | 7 | 14 | 30 };
  }
  if (v.kind === "rolling" && Number(v.hours) === 24) return { kind: "rolling", hours: 24 };
  if (v.kind === "custom" && localMinute(v.start)) {
    if (v.end === "now") return { kind: "custom", start: v.start, end: "now" };
    if (localMinute(v.end)) return { kind: "custom", start: v.start, end: v.end };
  }
  return null;
}

function isUntilNow(preset: RangePreset | null): boolean {
  return !!preset && preset.kind === "custom" && preset.end === "now";
}

/** 预设名（不依赖后端回传）。 */
export function presetName(preset: RangePreset | null): string {
  if (!preset || preset.kind === "all") return "全部时间";
  if (preset.kind === "days") return preset.days === 1 ? "当日" : `近 ${preset.days} 天`;
  if (preset.kind === "rolling") return "过去 24 小时";
  return "自定义范围";
}

const dayOf = (local: string): string => local.slice(0, 10);
const clockOf = (local: string): string => local.slice(11, 16);

function needYear(fromDay: string, toDay: string | null, today: string): boolean {
  const currentYear = yearOf(today);
  return yearOf(fromDay) !== currentYear || (toDay != null && yearOf(toDay) !== currentYear);
}

/** 日期部分：当前年用 M/D，非当前年/跨年用 yyyy/M/d（沿用 rangeLabel 规则）。 */
function dayLabel(day: string, useFull: boolean): string {
  return useFull ? fullDate(day) : shortDate(day);
}

/**
 * 按钮标签（32px 触发器上的语义文字）。
 * 自然日预设与 24h 与后端无关；自定义形态依赖 range，缺失时降级"自定义范围"。
 */
export function presetLabel(
  preset: RangePreset | null,
  range: RangeView | null | undefined,
  tz: string,
): string {
  if (!preset || preset.kind === "all") return "全部时间";
  if (preset.kind === "days") return preset.days === 1 ? "当日" : `${preset.days}d`;
  if (preset.kind === "rolling") return "24h";
  if (!range) return "自定义范围";
  const today = todayInTz(tz);
  if (range.granularity === "day") {
    const from = range.from;
    if (!from) return "自定义范围";
    const untilNow = isUntilNow(preset);
    const full = needYear(from, untilNow ? null : range.to, today);
    if (untilNow || !range.to) return `${dayLabel(from, full)} 起`;
    if (from === range.to) return dayLabel(from, full);
    return `${dayLabel(from, full)}–${dayLabel(range.to, full)}`;
  }
  const startDay = dayOf(range.start_local);
  const endDay = dayOf(range.end_local);
  const untilNow = isUntilNow(preset);
  const full = needYear(startDay, untilNow ? null : endDay, today);
  const head = `${dayLabel(startDay, full)} ${clockOf(range.start_local)}`;
  if (untilNow) return `${head} 起`;
  // 同年内的同一天省略重复日期，跨天两端都写（含年份时两端都写全）
  if (!full && startDay === endDay) return `${head}–${clockOf(range.end_local)}`;
  return `${head}–${dayLabel(endDay, full)} ${clockOf(range.end_local)}`;
}

/**
 * 悬停提示与可访问名称：完整范围。
 * 优先取后端回传的 range；后端尚未返回时只写预设名。
 */
export function presetAriaLabel(
  preset: RangePreset | null,
  range: RangeView | null | undefined,
  tz: string,
): string {
  const name = presetName(preset);
  if (!range) return name;
  const today = todayInTz(tz);
  if (range.granularity === "day") {
    if (!range.from) return name;
    const untilNow = isUntilNow(preset);
    const full = needYear(range.from, untilNow ? null : range.to, today);
    if (untilNow || !range.to) return `${name}：${dayLabel(range.from, full)} 起`;
    if (range.from === range.to) return `${name}：${dayLabel(range.from, full)}`;
    return `${name}：${dayLabel(range.from, full)} – ${dayLabel(range.to, full)}`;
  }
  const startDay = dayOf(range.start_local);
  const endDay = dayOf(range.end_local);
  const untilNow = isUntilNow(preset);
  const full = needYear(startDay, untilNow ? null : endDay, today);
  const head = `${dayLabel(startDay, full)} ${clockOf(range.start_local)}`;
  if (untilNow) return `${name}：${head} 起`;
  return `${name}：${head} – ${dayLabel(endDay, full)} ${clockOf(range.end_local)}`;
}
