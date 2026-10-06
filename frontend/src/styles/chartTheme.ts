/// ECharts 主题适配器（设计系统 Task 1）。
/// 固定四类 token 的语义色与顺序（输入/输出/缓存写/缓存读），禁止使用
/// ECharts 默认调色板；网格、坐标轴、文字与 tooltip 颜色全部从语义
/// token 派生。色值与 tokens.css 的 --ts-chart-* 同步维护。
import type { ThemeMode } from "../composables/theme";

export type TokenKind = "input" | "output" | "cache_write" | "cache_read";

export interface ChartSeriesToken {
  key: TokenKind;
  label: string;
  color: string;
}

export interface ChartTokens {
  series: ChartSeriesToken[];
  backgroundColor: string;
  text: string;
  textSecondary: string;
  textMuted: string;
  border: string;
  splitLine: string;
  tooltipBg: string;
  tooltipBorder: string;
  legendText: string;
}

const SERIES_META: [TokenKind, string][] = [
  ["input", "输入"],
  ["output", "输出"],
  ["cache_write", "缓存写"],
  ["cache_read", "缓存读"],
];

/// 与 tokens.css --ts-chart-* 同步的系列色。
const SERIES_COLORS: Record<ThemeMode, Record<TokenKind, string>> = {
  light: {
    input: "#3D7EA6",
    output: "#C2732B",
    cache_write: "#4E9B6E",
    cache_read: "#7A8CA0",
  },
  dark: {
    input: "#7FB3D8",
    output: "#E0A06A",
    cache_write: "#7CC49A",
    cache_read: "#9FB3C8",
  },
};

export function chartTokens(mode: ThemeMode): ChartTokens {
  const dark = mode === "dark";
  const colors = SERIES_COLORS[mode];
  return {
    series: SERIES_META.map(([key, label]) => ({
      key,
      label,
      color: colors[key],
    })),
    backgroundColor: "transparent",
    text: dark ? "#EDF5FA" : "#17232E",
    textSecondary: dark ? "#C0CFDA" : "#405362",
    textMuted: dark ? "#91A5B4" : "#647786",
    border: dark ? "#2A3B4B" : "#D4DEE6",
    splitLine: dark ? "rgba(42, 59, 75, 0.55)" : "rgba(212, 222, 230, 0.6)",
    tooltipBg: dark ? "rgba(25, 38, 52, 0.96)" : "rgba(255, 255, 255, 0.94)",
    tooltipBorder: dark ? "#3A5062" : "#B8CAD5",
    legendText: dark ? "#C0CFDA" : "#405362",
  };
}

/// 供 echarts.init(theme) / registerTheme 使用的主题对象（无默认调色板）。
export function echartsThemeObject(mode: ThemeMode) {
  const t = chartTokens(mode);
  return {
    color: t.series.map((s) => s.color),
    backgroundColor: t.backgroundColor,
    textStyle: { fontFamily: "inherit", color: t.text },
    legend: { textStyle: { color: t.legendText } },
    categoryAxis: {
      axisLine: { lineStyle: { color: t.border } },
      axisTick: { lineStyle: { color: t.border } },
      axisLabel: { color: t.textSecondary },
      splitLine: { show: false },
    },
    valueAxis: {
      axisLine: { show: false },
      axisLabel: { color: t.textMuted },
      splitLine: { lineStyle: { color: t.splitLine } },
    },
    tooltip: {
      backgroundColor: t.tooltipBg,
      borderColor: t.tooltipBorder,
      borderWidth: 1,
      textStyle: { color: t.text },
    },
  };
}
