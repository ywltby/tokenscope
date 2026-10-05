// 计划 C3（时区语义）：区间毫秒 → 解析时区下的 YYYY-MM-DD。
// 用 Intl 按所选 IANA 时区的**当日实际偏移**换算（DST 正确），替代
// 修复前 "local 用本地格式化、其余硬编码 +8" 的实现（UTC 之外的固定
// 偏移时区与非 +8 时区都会错日）。
export function tzDate(ms: number, tz: string): string {
  if (tz === "local") return new Date(ms).toLocaleDateString("sv-SE");
  return new Intl.DateTimeFormat("sv-SE", { timeZone: tz }).format(new Date(ms));
}
