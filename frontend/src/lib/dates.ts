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
