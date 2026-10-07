/// ECharts 主题适配器（设计系统 Task 1，苹果风格）。
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
  separator: string;
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
    input: "#4C8DF6",
    output: "#F2A24A",
    cache_write: "#5BBF7A",
    cache_read: "#A7AEB8",
  },
  dark: {
    input: "#64A0FF",
    output: "#FFB35C",
    cache_write: "#6FD38D",
    cache_read: "#8E949C",
  },
};

export function chartTokens(mode: ThemeMode): ChartTokens {
  const dark = mode === "dark";
  const colors = SERIES_COLORS[mode];
  // UX01（DESIGN.md §2/§5）：网格线映射 --ts-separator；tooltip 用 elevated
  // 浮层（85%），与 Naive 浮层同一配方。
  const separator = dark ? "rgba(255, 255, 255, 0.08)" : "rgba(0, 0, 0, 0.08)";
  return {
    series: SERIES_META.map(([key, label]) => ({
      key,
      label,
      color: colors[key],
    })),
    backgroundColor: "transparent",
    text: dark ? "#F5F5F7" : "#1D1D1F",
    textSecondary: dark ? "#AEAEB2" : "#515154",
    textMuted: dark ? "#8E8E93" : "#6E6E73",
    separator,
    splitLine: separator,
    tooltipBg: dark ? "rgba(44, 44, 46, 0.85)" : "rgba(255, 255, 255, 0.85)",
    tooltipBorder: dark ? "rgba(255, 255, 255, 0.16)" : "rgba(0, 0, 0, 0.14)",
    legendText: dark ? "#AEAEB2" : "#515154",
  };
}

/// UX01：浮层模糊配方（16px）——ECharts HTML tooltip 经 extraCssText 消费，
/// 与 Naive Popover/Tooltip 的 backdrop-filter 配方同值。
export const POPOVER_BLUR_CSS =
  "backdrop-filter: blur(16px); -webkit-backdrop-filter: blur(16px); border-radius: 12px;";

/// 供 echarts.init(theme) / registerTheme 使用的主题对象（无默认调色板）。
export function echartsThemeObject(mode: ThemeMode) {
  const t = chartTokens(mode);
  return {
    color: t.series.map((s) => s.color),
    backgroundColor: t.backgroundColor,
    textStyle: { fontFamily: "inherit", color: t.text },
    legend: { textStyle: { color: t.legendText, fontSize: 12 } },
    categoryAxis: {
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: t.textMuted, fontSize: 12 },
      splitLine: { show: false },
    },
    valueAxis: {
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: t.textMuted, fontSize: 12 },
      splitLine: { lineStyle: { color: t.splitLine, type: "solid" } },
    },
    tooltip: {
      backgroundColor: t.tooltipBg,
      borderColor: t.tooltipBorder,
      borderWidth: 1,
      textStyle: { color: t.text },
      padding: [12, 16],
    },
  };
}
