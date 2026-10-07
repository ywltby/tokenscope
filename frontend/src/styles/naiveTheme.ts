/// Naive UI 主题覆盖适配器（设计系统 Task 1，苹果风格）。
/// 色值与 tokens.css 同步维护（来源 DESIGN.md 第二版）——CSS 变量无法在
/// 渲染前同步读取，故这里以 TS 映射承载同一套语义色。
import type { GlobalThemeOverrides } from "naive-ui";
import type { ThemeMode } from "../composables/theme";

interface TsPalette {
  canvas: string;
  /** 玻璃卡片底色（半透明，与 tokens.css --ts-surface 同步） */
  surfaceGlass: string;
  /** 实色数据区（表格），与 tokens.css --ts-surface-solid 同步 */
  surface: string;
  elevated: string;
  fill: string;
  fillHover: string;
  separator: string;
  separatorStrong: string;
  text: string;
  textSecondary: string;
  textMuted: string;
  accent: string;
  accentFill: string;
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
    canvas: "#F5F5F7",
    surfaceGlass: "rgba(255, 255, 255, 0.75)",
    surface: "#FFFFFF",
    // UX01：浮层外壳 = 85% elevated（与 tokens.css --ts-surface-elevated 一致）
    elevated: "rgba(255, 255, 255, 0.85)",
    fill: "rgba(118, 118, 128, 0.12)",
    fillHover: "rgba(118, 118, 128, 0.08)",
    separator: "rgba(0, 0, 0, 0.08)",
    separatorStrong: "rgba(0, 0, 0, 0.14)",
    text: "#1D1D1F",
    textSecondary: "#515154",
    textMuted: "#6E6E73",
    accent: "#0066CC",
    accentFill: "#0071E3",
    accentSoft: "rgba(0, 113, 227, 0.1)",
    onAccent: "#FFFFFF",
    success: "#1E7B34",
    warning: "#B25000",
    error: "#D70015",
    info: "#2F6FB0",
  },
  dark: {
    canvas: "#0F0F11",
    surfaceGlass: "rgba(28, 28, 30, 0.75)",
    surface: "#1C1C1E",
    elevated: "rgba(44, 44, 46, 0.85)",
    fill: "rgba(118, 118, 128, 0.24)",
    fillHover: "rgba(118, 118, 128, 0.16)",
    separator: "rgba(255, 255, 255, 0.08)",
    separatorStrong: "rgba(255, 255, 255, 0.16)",
    text: "#F5F5F7",
    textSecondary: "#AEAEB2",
    textMuted: "#8E8E93",
    accent: "#4DA3FF",
    accentFill: "#0060DF",
    accentSoft: "rgba(77, 163, 255, 0.16)",
    onAccent: "#FFFFFF",
    success: "#30D158",
    warning: "#FF9F0A",
    error: "#FF6961",
    info: "#64D2FF",
  },
};

/// Naive UI themeOverrides：body、card、表格、输入、按钮、tooltip、
/// popover、alert、tabs 的表面/文字/边界/圆角统一走语义色。
export function naiveThemeOverrides(mode: ThemeMode): GlobalThemeOverrides {
  const c = PALETTES[mode];
  const isDark = mode === "dark";
  // 浮层规范阴影（DESIGN.md §2：浅色 0 12px 48px rgba(0,0,0,.15) / 深色 .6）
  const shadowElevated = isDark
    ? "0 12px 48px rgba(0, 0, 0, 0.6)"
    : "0 12px 48px rgba(0, 0, 0, 0.15)";
  return {
    common: {
      // 画布透明：body 的氛围光斑由 tokens.css 绘制，NGlobalStyle 不得盖掉
      bodyColor: "transparent",
      // NCard 走玻璃底色（模糊由 .ts-card 提供）；表格仍用实色 surface 保证可读
      cardColor: c.surfaceGlass,
      modalColor: c.elevated,
      popoverColor: c.elevated,
      tableColor: c.surface,
      tableHeaderColor: c.surface,
      inputColor: c.fill,
      actionColor: c.fill,
      hoverColor: c.fillHover,
      textColorBase: c.text,
      textColor1: c.text,
      textColor2: c.textSecondary,
      textColor3: c.textMuted,
      borderColor: c.separator,
      dividerColor: c.separator,
      primaryColor: c.accent,
      primaryColorHover: c.accent,
      primaryColorPressed: c.accent,
      primaryColorSuppl: c.accentFill,
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
      borderRadiusSmall: "8px",
      fontWeightStrong: "600",
      heightMedium: "32px",
      heightSmall: "28px",
    },
    Card: {
      borderRadius: "14px",
      borderColor: isDark ? c.separator : "transparent",
      padding: "20px",
    },
    Button: {
      borderRadiusMedium: "8px",
      borderRadiusSmall: "8px",
      borderRadiusTiny: "8px",
      border: "none",
      paddingMedium: "0 16px",
      paddingSmall: "0 12px",
    },
    Input: {
      borderRadius: "8px",
      border: `1px solid ${c.separatorStrong}`,
      heightMedium: "32px",
      heightSmall: "28px",
    },
    Select: {
      peers: {
        InternalSelection: {
          borderRadius: "8px",
          border: "none",
          heightMedium: "32px",
          heightSmall: "28px",
        },
      },
    },
    DataTable: {
      // 任务 5：无竖线无外框（bordered=false + single-line 默认），
      // 行间保留 separator 发丝线（DESIGN.md §5 表格）
      borderColor: c.separator,
      borderRadius: "0",
      thColor: c.surface,
      tdColor: c.surface,
      tdColorHover: c.fillHover,
      thPaddingMedium: "12px 16px",
      thPaddingSmall: "10px 12px",
      tdPaddingMedium: "12px 16px",
      tdPaddingSmall: "10px 12px",
      // UX01（DESIGN.md §3/§5）：表格正文 13px/1.45/primary，表头 12px/500/
      // secondary。仅局部覆盖 DataTable，不动 common.fontSizeSmall。
      fontSizeSmall: "13px",
      lineHeight: "1.45",
      thFontWeight: "500",
      thTextColor: c.textSecondary,
      tdTextColor: c.text,
    },
    Tooltip: {
      color: c.elevated,
      textColor: c.text,
      borderRadius: "12px",
      padding: "12px 16px",
      boxShadow: shadowElevated,
    },
    Popover: {
      color: c.elevated,
      textColor: c.text,
      borderRadius: "12px",
      padding: "12px 16px",
      boxShadow: shadowElevated,
    },
    Alert: {
      borderRadius: "12px",
      padding: "12px 16px",
    },
    Tabs: { tabBorderColor: "transparent" },
  };
}
