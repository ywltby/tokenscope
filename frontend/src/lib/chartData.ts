// 图表数据装配（计划 A4 / F03）：分类轴与全部 series 必须由同一份排序
// 结果生成——此前非日维度只对分类标签按用量降序，series 仍用原始顺序，
// 标签与数值错位（A=10、B=100 可能显示 B=10、A=100）。
import { TOKEN_BUCKETS } from "./tokenDisplay";
import type { Group, TokenCounts } from "../types";
import { displayTokens, totalTokens } from "./tokenUsage";

// UX07：显示名消费 tokenDisplay 单一来源（U14：缓存命中）。
export const SERIES: { name: keyof TokenCounts; label: string }[] = TOKEN_BUCKETS.map((b) => ({
  name: b.key,
  label: b.label,
}));

export interface BarChartSeries {
  name: string;
  values: number[];
}

export interface BarChartData {
  categories: string[];
  series: BarChartSeries[];
}

/// UX05：真实可绘制类别——排除"合计"行。合计是汇总行，不是维度类别；
/// 父级用 computed 缓存本结果，避免在模板里每次 filter 生成新数组
/// （新数组会改变 props 身份，触发图表不必要的重建）。
export function realGroups(groups: Group[]): Group[] {
  return groups.filter((g) => g.key !== "合计");
}

/// 日维度保持时间序；其余按总输入 + 输出降序，缓存不重复计数。
function orderGroups(groups: Group[], by: string): Group[] {
  const total = (g: Group): number => totalTokens(g.tokens);
  return by === "day" ? groups : [...groups].sort((a, b) => total(b) - total(a));
}

export function buildBarChartData(groups: Group[], by: string): BarChartData {
  const ordered = orderGroups(groups, by);
  return {
    // C2：项目维度分类显示用展示名（末段），完整路径经 key 保留。
    categories: ordered.map((g) => g.label ?? g.key),
    series: SERIES.map((s) => ({
      name: s.label,
      values: ordered.map((g) => displayTokens(g.tokens)[s.name]),
    })),
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
