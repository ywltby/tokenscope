// UX07：金额格式化单一入口——请求金额、汇总金额与每百万 token 单价
// 三种场景显式区分，显示层只做舍入排版，格式化字符串不参与计算
//（不变量 7：金额一律来自后端）。未知（null/undefined）由调用点按
// 类型明确传递并显示"未知"，不用 0 代替（zero_and_unknown_are_distinct）。
//
// 规则（RC07：scenario **真正进入分支**，不再被丢弃）：
// - `unit`（USD / 1M token 单价）：保留 JS number 的**最短可往返十进制
//   有效表示**（以 `String(value)` 为基础），不额外强制两/四/六位舍入——
//   1.234567 必须显示 `$1.234567` 而不是 `$1.23`（2026-10-08 复核 RC07：
//   旧实现丢弃 scenario 并对 >= 1 的值固定两位小数，真实单价精度丢失）；
//   极小/极大值由 JS 给出清晰科学记数法（如 `$1e-8`），明确非零；
// - `request` / `summary`：保留既有展示策略（0 为 `$0.00`；0 < v < 1 至少
//   四位小数，更小按需六位；六位舍入后仍为零 → 科学记数法，绝不把非零值
//   显示成 `$0.00`；≥ 1 两位习惯精度）；
// - 0 在两种策略下都明确为 `$0.00`（免费 ≠ 未知）。
//
// 返回完整 USD 文本（含 $）——调用方不得再次拼接货币符号。

export type MoneyScenario = "request" | "summary" | "unit";

export function formatMoney(
  v: number | null | undefined,
  scenario: MoneyScenario = "request",
): string {
  if (v == null || Number.isNaN(v)) return "未知";
  if (!Number.isFinite(v)) return "未知";
  if (v === 0) return "$0.00";
  const abs = Math.abs(v);
  const sign = v < 0 ? "-" : "";
  if (scenario === "unit") {
    // RC07：最短可往返十进制有效表示——`String(abs)` 即 JS number 的最短
    // 往返表示，不做额外舍入；科学记数法（1e-8 / 1e+21）是合法且清晰的
    // 非零表达。
    return `${sign}$${abs}`;
  }
  // 微小非零：六位小数舍入后仍为零 → 科学记数法（明确非零表达）。
  if (abs < 0.001) {
    const six = abs.toFixed(6);
    if (Number(six) === 0) return `${sign}$${abs.toExponential(2)}`;
    return `${sign}$${six}`;
  }
  if (abs < 1) return `${sign}$${abs.toFixed(4)}`;
  return `${sign}$${abs.toFixed(2)}`;
}
