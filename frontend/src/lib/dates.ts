// 计划 C3（时区语义）：区间毫秒 → 解析时区下的 YYYY-MM-DD。
// 用 Intl 按所选 IANA 时区的**当日实际偏移**换算（DST 正确），替代
// 修复前 "local 用本地格式化、其余硬编码 +8" 的实现（UTC 之外的固定
// 偏移时区与非 +8 时区都会错日）。
export function tzDate(ms: number, tz: string): string {
  if (tz === "local") return new Date(ms).toLocaleDateString("sv-SE");
  return new Intl.DateTimeFormat("sv-SE", { timeZone: tz }).format(new Date(ms));
}

/// 指定时区的"今天"（YYYY-MM-DD 日历值）。
/// R03：`local` 不能传给 Intl（RangeError）——特判为本机日历日。
export function todayInTz(tz: string): string {
  if (tz === "local") return new Date().toLocaleDateString("sv-SE");
  return new Intl.DateTimeFormat("sv-SE", { timeZone: tz }).format(new Date());
}

/// 日历日加减 n 天（纯日历运算，不经本机时刻，DST 安全）。
export function addDays(dateStr: string, n: number): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const t = Date.UTC(y, m - 1, d) + n * 86400e3;
  return new Date(t).toISOString().slice(0, 10);
}

// ── UX04：日期区间标签（纯函数，便于单测） ────────────────────────
//
// 修复前只用 M/D 短标签且用短标签比较是否同日——2024-01-01..2025-01-01
// 与 2024-03-05 这类历史区间会丢失年份、甚至被折叠成"1/1"。规则：
//   - 任一端点年份不是统计时区当前年，或区间跨年 → 带年份（yyyy/M/d）；
//   - 同年且为当前年 → 保留简短 M/D；
//   - "同日"用完整 ISO 日期比较，不用 M/D 标签比较。

/** 统计时区当前年（YYYY）。 */
export function yearOf(dateStr: string): string {
  return dateStr.slice(0, 4);
}

/** 简短标签 M/D（同年当年用）。 */
export function shortDate(dateStr: string): string {
  const [, m, d] = dateStr.split("-");
  return `${Number(m)}/${Number(d)}`;
}

/** 带年份标签 yyyy/M/d。 */
export function fullDate(dateStr: string): string {
  const y = yearOf(dateStr);
  return `${y}/${Number(dateStr.slice(5, 7))}/${Number(dateStr.slice(8, 10))}`;
}

/**
 * 区间标签：`from ~ to`（跟随今天时右端显示"今天"）。
 * 同日（完整 ISO 相等）只显示一个端点；跨年/非当前年展示足够年份。
 */
export function rangeLabel(from: string, to: string, today: string): string {
  const currentYear = yearOf(today);
  const needYear = yearOf(from) !== currentYear || yearOf(to) !== currentYear;
  const fmt = (s: string): string => (needYear ? fullDate(s) : shortDate(s));
  if (to >= today && from < today) return `${fmt(from)} ~ 今天`;
  if (from === to) return fmt(from);
  return `${fmt(from)} ~ ${fmt(to)}`;
}
