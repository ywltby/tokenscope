// 图表数据装配（计划 A4 / F03）：分类轴与全部 series 必须由同一份排序
// 结果生成——此前非日维度只对分类标签按用量降序，series 仍用原始顺序，
// 标签与数值错位（A=10、B=100 可能显示 B=10、A=100）。
import { TOKEN_BUCKETS } from "./tokenDisplay";
import type { Group, TokenCounts } from "../types";
import { bigIntToScaledNumber, toTokenBigInt, totalTokens } from "./tokenUsage";

// UX07：显示名消费 tokenDisplay 单一来源（U14：缓存命中）。
export const SERIES: { name: keyof TokenCounts; label: string }[] = TOKEN_BUCKETS.map((b) => ({
  name: b.key,
  label: b.label,
}));

export interface BarChartSeries {
  name: string;
  /** 供 ECharts 绘制柱高/柱长的有限数值（超出 10^290 时按 `scalePow10` 统一缩放）。 */
  values: number[];
  /** 原始任意精度 BigInt，供悬浮 tooltip 展示精确整数，不受 f64 尾数截断影响。 */
  rawValues: bigint[];
}

export interface BarChartData {
  categories: string[];
  series: BarChartSeries[];
  /** ECharts 数值统一缩小的 10 的幂次（常规 <= 10^290 时恒为 0）。 */
  scalePow10: number;
}

/// UX05：真实可绘制类别——排除"合计"行。合计是汇总行，不是维度类别；
/// 父级用 computed 缓存本结果，避免在模板里每次 filter 生成新数组
/// （新数组会改变 props 身份，触发图表不必要的重建）。
export function realGroups(groups: Group[]): Group[] {
  return groups.filter((g) => g.key !== "合计");
}

/// 日维度保持时间序；其余按总输入 + 输出（原生 BigInt）精确降序，缓存不重复计数。
function orderGroups(groups: Group[], by: string): Group[] {
  if (by === "day") return groups;
  return [...groups].sort((a, b) => {
    const tb = totalTokens(b.tokens);
    const ta = totalTokens(a.tokens);
    return tb > ta ? 1 : tb < ta ? -1 : 0;
  });
}

export function buildBarChartData(groups: Group[], by: string): BarChartData {
  const ordered = orderGroups(groups, by);
  let maxTotal = 0n;
  for (const g of ordered) {
    const t = totalTokens(g.tokens);
    if (t > maxTotal) maxTotal = t;
  }
  const maxDigits = maxTotal > 0n ? maxTotal.toString().length : 0;
  // 留足余量避免堆叠总和逼近 IEEE-754 f64::MAX (~1.8e308) 溢出为 Infinity
  const scalePow10 = maxDigits > 290 ? maxDigits - 290 : 0;
  return {
    // 每个后端分组直接绘制一个类别，不在图表层按名字再分组。展示名可重复；
    // 统计分组键由 Rust 同时交给表格与图表。
    categories: ordered.map((g) => g.label ?? g.key),
    // 2026-10-10 用户修订：图表与英雄栏统一用**互斥四桶**——「输入」= 未缓存
    // 输入（不含缓存写入/缓存命中），四类相加 = 总 token，不重复计数。
    // 排序仍按总 token（四桶之和，与展示口径一致）。
    series: SERIES.map((s) => {
      const rawValues = ordered.map((g) => toTokenBigInt(g.tokens[s.name]));
      return {
        name: s.label,
        rawValues,
        values: rawValues.map((v) => bigIntToScaledNumber(v, scalePow10)),
      };
    }),
    scalePow10,
  };
}

/// 设计系统 Task 4：与分类轴同序的完整原始键（长名 tooltip 用，不参与聚合）。
export function fullLabels(groups: Group[], by: string): string[] {
  return orderGroups(groups, by).map((g) => g.key);
}

/// MP04：tooltip 标题——项目维度保留完整路径（长名靠 tooltip 展开），其余
/// 维度显示展示名：模型维度的 key 是等价身份键（`claudeopus55`），绝不能拿它
/// 当标题；原始模型名仍在请求明细里可查。
export function tooltipLabels(groups: Group[], by: string): string[] {
  return orderGroups(groups, by).map((g) => (by === "project" ? g.key : (g.label ?? g.key)));
}
