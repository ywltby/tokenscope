// 图表数据装配（计划 A4 / F03）：分类轴与全部 series 必须由同一份排序
// 结果生成——此前非日维度只对分类标签按用量降序，series 仍用原始顺序，
// 标签与数值错位（A=10、B=100 可能显示 B=10、A=100）。
import type { Group, TokenCounts } from "../types";

export const SERIES: { name: keyof TokenCounts; label: string }[] = [
  { name: "input", label: "输入" },
  { name: "output", label: "输出" },
  { name: "cache_write", label: "缓存写" },
  { name: "cache_read", label: "缓存读" },
];

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

/// 分类顺序（单一事实源）：日维度保持时间序；非日维度按 输入+输出 用量降序。
function orderGroups(groups: Group[], by: string): Group[] {
  return by === "day"
    ? groups
    : [...groups].sort(
        (a, b) => b.tokens.input + b.tokens.output - (a.tokens.input + a.tokens.output),
      );
}

export function buildBarChartData(groups: Group[], by: string): BarChartData {
  const ordered = orderGroups(groups, by);
  return {
    // C2：项目维度分类显示用展示名（末段），完整路径经 key 保留。
    categories: ordered.map((g) => g.label ?? g.key),
    series: SERIES.map((s) => ({
      name: s.label,
      values: ordered.map((g) => g.tokens[s.name]),
    })),
  };
}

/// 设计系统 Task 4：与分类轴同序的完整原始键（长名 tooltip 用，不参与聚合）。
export function fullLabels(groups: Group[], by: string): string[] {
  return orderGroups(groups, by).map((g) => g.key);
}

/// 可访问文字摘要：与图表数据等价的逐类别行（图例/颜色之外的读数路径）。
export function chartSummaryLines(groups: Group[], by: string): string[] {
  const title =
    by === "day"
      ? "按日汇总："
      : by === "model"
        ? "按模型汇总："
        : by === "project"
          ? "按项目汇总："
          : "按应用汇总：";
  const lines = [title];
  for (const g of orderGroups(groups, by)) {
    const t = g.tokens;
    const total = t.input + t.output + t.cache_write + t.cache_read;
    lines.push(
      `${g.label ?? g.key}：输入 ${t.input} · 输出 ${t.output} · 缓存写 ${t.cache_write} · 缓存读 ${t.cache_read} · 合计 ${total}`,
    );
  }
  return lines;
}

/// 类别数量状态：空/单/多都有明确文案（多类别说明可滚动查看全部）。
export function chartStateText(count: number): string {
  if (count === 0) return "暂无数据：调整时间范围或来源后重试";
  if (count === 1) return "仅 1 个类别";
  return `${count} 个类别，图内可滚动查看全部`;
}
