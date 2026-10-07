// UX07：金额格式化单一入口——请求金额、汇总金额与每百万 token 单价
// 三种场景显式区分，显示层只做舍入排版，格式化字符串不参与计算
//（不变量 7：金额一律来自后端）。未知（null/undefined）由调用点按
// 类型明确传递并显示"未知"，不用 0 代替（zero_and_unknown_are_distinct）。
//
// 规则（request/summary/unit 同族，均保护微小非零值）：
// - 0 明确为 $0.00（免费 ≠ 未知）；
// - 0 < v < 1：至少四位小数，小值（< 0.001）按需六位；
// - 若六位舍入后仍为零 → 可读科学记数法（如 $1.00e-8），
//   绝不把非零值显示成 $0.00 / $0；
// - ≥ 1：两位习惯精度；summary 场景同样保护微小非零值；
// - unit 场景（USD / 百万 token）保留有效精度，单位注明由调用方文案负责。
//
// 返回完整 USD 文本（含 $）——调用方不得再次拼接货币符号。

export type MoneyScenario = "request" | "summary" | "unit";

export function formatMoney(
  v: number | null | undefined,
  // 场景目前共享同一族规则；显式参数固化调用点语义（请求/汇总/单价），
  // 后续场景差异只改本入口。
  scenario: MoneyScenario = "request",
): string {
  void scenario;
  if (v == null || Number.isNaN(v)) return "未知";
  if (!Number.isFinite(v)) return "未知";
  if (v === 0) return "$0.00";
  const abs = Math.abs(v);
  const sign = v < 0 ? "-" : "";
  // 微小非零：六位小数舍入后仍为零 → 科学记数法（明确非零表达）。
  if (abs < 0.001) {
    const six = abs.toFixed(6);
    if (Number(six) === 0) return `${sign}$${abs.toExponential(2)}`;
    return `${sign}$${six}`;
  }
  if (abs < 1) return `${sign}$${abs.toFixed(4)}`;
  return `${sign}$${abs.toFixed(2)}`;
}
