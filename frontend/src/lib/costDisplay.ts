// v0.9.3：费用显示状态与文案的单一来源。
//
// 只决定「怎么显示」：金额与 unknown_tokens 全部来自后端 DTO，前端不重算。
// 费用列、合计行与总览卡共用本模块，保证同一组在三处颜色/文案一致。
import type { Group, TokenCounts } from "../types";
import { formatMoney } from "./formatMoney";

/// 组内实际未获费用覆盖的四桶 token 总数（分档已知标记不参与）。
export function unknownTokenTotal(u: TokenCounts): number {
  return u.input + u.output + u.cache_write + u.cache_read;
}

/**
 * 费用显示色态：只有真实存在未定价用量时才用警告色。
 * - 全部未定价（total_cost_usd=null）→ warning + 「未知」；
 * - 部分未定价（有金额且 unknown>0）→ warning + 橙色金额；
 * - unknown=0（含仅有「缺价按零估算」标记）→ normal，零价显示 `$0.00`。
 */
export function costDisplayTone(g: Group): "normal" | "warning" {
  return unknownTokenTotal(g.unknown_tokens) > 0 ? "warning" : "normal";
}

/// 单一金额：后端合并值；没有费用依据时显示调用方给出的「未知」文案。
export function costDisplayText(g: Group, unknownText: string): string {
  return g.total_cost_usd != null ? formatMoney(g.total_cost_usd, "summary") : unknownText;
}
