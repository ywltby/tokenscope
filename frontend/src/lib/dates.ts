// 计划 C3（时区语义）：区间毫秒 → 解析时区下的 YYYY-MM-DD。
// 用 Intl 按所选 IANA 时区的**当日实际偏移**换算（DST 正确），替代
// 修复前 "local 用本地格式化、其余硬编码 +8" 的实现（UTC 之外的固定
// 偏移时区与非 +8 时区都会错日）。
export function tzDate(ms: number, tz: string): string {
  if (tz === "local") return new Date(ms).toLocaleDateString("sv-SE");
  return new Intl.DateTimeFormat("sv-SE", { timeZone: tz }).format(new Date(ms));
}

/// 指定时区的"今天"（YYYY-MM-DD 日历值）。
export function todayInTz(tz: string): string {
  return new Intl.DateTimeFormat("sv-SE", { timeZone: tz }).format(new Date());
}

/// 日历日加减 n 天（纯日历运算，不经本机时刻，DST 安全）。
export function addDays(dateStr: string, n: number): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const t = Date.UTC(y, m - 1, d) + n * 86400e3;
  return new Date(t).toISOString().slice(0, 10);
}

/// 日历字符串 → NDatePicker 所需毫秒（锚定 UTC 零点，仅作组件输入）。
export function calendarToMs(dateStr: string): number {
  const [y, m, d] = dateStr.split("-").map(Number);
  return Date.UTC(y, m - 1, d);
}

/// 毫秒 → UTC 锚定日历日期（YYYY-MM-DD）。
/// 与 `calendarToMs` 互为逆运算：calendarToMs 用 UTC 零点锚定日历字符串，
/// 这里用 UTC 日历提取还原——picker 的 UI 毫秒值只承担桥接语义，
/// 不得用统计时区重解释（审阅 Task 1）。
export function msToUtcCalendar(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}
