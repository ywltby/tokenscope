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

export function buildBarChartData(groups: Group[], by: string): BarChartData {
  // 日维度保持时间序（后端按日升序）；非日维度按 输入+输出 用量降序。
  const ordered =
    by === "day"
      ? groups
      : [...groups].sort(
          (a, b) => b.tokens.input + b.tokens.output - (a.tokens.input + a.tokens.output),
        );
  return {
    // C2：项目维度分类显示用展示名（末段），完整路径经 key 保留。
    categories: ordered.map((g) => g.label ?? g.key),
    series: SERIES.map((s) => ({
      name: s.label,
      values: ordered.map((g) => g.tokens[s.name]),
    })),
  };
}
