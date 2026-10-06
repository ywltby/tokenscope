/// Naive UI 主题覆盖适配器（设计系统 Task 1）。
/// 色值与 tokens.css 同步维护（来源 DESIGN.md §2 语义色表）——CSS 变量
/// 无法在渲染前同步读取，故这里以 TS 映射承载同一套语义色。
import type { GlobalThemeOverrides } from "naive-ui";
import type { ThemeMode } from "../composables/theme";

interface TsPalette {
  canvas: string;
  surface: string;
  surfaceSolid: string;
  elevated: string;
  border: string;
  borderStrong: string;
  text: string;
  textSecondary: string;
  textMuted: string;
  accent: string;
  accentHover: string;
  accentSoft: string;
  onAccent: string;
  success: string;
  warning: string;
  error: string;
  info: string;
}

/// 与 tokens.css 的浅/深两组值一一对应。
const PALETTES: Record<ThemeMode, TsPalette> = {
  light: {
    canvas: "#EEF3F7",
    surface: "rgba(255, 255, 255, 0.82)",
    surfaceSolid: "#F8FAFC",
    elevated: "rgba(255, 255, 255, 0.94)",
    border: "#D4DEE6",
    borderStrong: "#B8CAD5",
    text: "#17232E",
    textSecondary: "#405362",
    textMuted: "#647786",
    accent: "#087EA4",
    accentHover: "#056782",
    accentSoft: "#DDF3F8",
    onAccent: "#FFFFFF",
    success: "#19734A",
    warning: "#9A5B00",
    error: "#B33A3A",
    info: "#356B9A",
  },
  dark: {
    canvas: "#0B1118",
    surface: "rgba(19, 30, 42, 0.82)",
    surfaceSolid: "#121D29",
    elevated: "rgba(25, 38, 52, 0.96)",
    border: "#2A3B4B",
    borderStrong: "#3A5062",
    text: "#EDF5FA",
    textSecondary: "#C0CFDA",
    textMuted: "#91A5B4",
    accent: "#6FD3EE",
    accentHover: "#9AE5F5",
    accentSoft: "#173C4A",
    onAccent: "#08222C",
    success: "#72D6A1",
    warning: "#F4C56A",
    error: "#FF8F8F",
    info: "#8FC5F2",
  },
};

/// Naive UI themeOverrides：body、card、表格、输入、按钮、tooltip、
/// popover、alert、tabs 的表面/文字/边界/圆角统一走语义色。
export function naiveThemeOverrides(mode: ThemeMode): GlobalThemeOverrides {
  const c = PALETTES[mode];
  return {
    common: {
      bodyColor: c.canvas,
      cardColor: c.surface,
      modalColor: c.elevated,
      popoverColor: c.elevated,
      tableColor: c.surfaceSolid,
      tableHeaderColor: c.surfaceSolid,
      inputColor: c.surfaceSolid,
      actionColor: c.surfaceSolid,
      hoverColor: c.accentSoft,
      textColorBase: c.text,
      textColor1: c.text,
      textColor2: c.textSecondary,
      textColor3: c.textMuted,
      borderColor: c.border,
      dividerColor: c.border,
      primaryColor: c.accent,
      primaryColorHover: c.accentHover,
      primaryColorPressed: c.accentHover,
      primaryColorSuppl: c.accent,
      successColor: c.success,
      successColorHover: c.success,
      successColorPressed: c.success,
      successColorSuppl: c.success,
      warningColor: c.warning,
      warningColorHover: c.warning,
      warningColorPressed: c.warning,
      warningColorSuppl: c.warning,
      errorColor: c.error,
      errorColorHover: c.error,
      errorColorPressed: c.error,
      errorColorSuppl: c.error,
      infoColor: c.info,
      infoColorHover: c.info,
      infoColorPressed: c.info,
      infoColorSuppl: c.info,
      borderRadius: "8px",
      borderRadiusSmall: "6px",
      fontWeightStrong: "650",
    },
    Card: {
      borderRadius: "12px",
      borderColor: c.border,
    },
    Button: {
      borderRadiusMedium: "6px",
      borderRadiusSmall: "6px",
      borderRadiusTiny: "6px",
    },
    Input: { borderRadius: "6px" },
    DataTable: {
      borderColor: c.border,
      borderRadius: "8px",
      thColor: c.surfaceSolid,
      tdColor: c.surfaceSolid,
      tdColorHover: c.accentSoft,
    },
    Tooltip: {
      color: c.elevated,
      textColor: c.text,
      borderRadius: "8px",
    },
    Popover: { borderRadius: "8px" },
    Alert: { borderRadius: "8px" },
    Tabs: { tabBorderColor: c.border },
  };
}
