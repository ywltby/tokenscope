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
  rowHover: string;
  separator: string;
  separatorStrong: string;
  text: string;
  textSecondary: string;
  textMuted: string;
  accent: string;
  accentFill: string;
  accentFillHover: string;
  accentFillPressed: string;
  accentSoft: string;
  onAccent: string;
  success: string;
  warning: string;
  error: string;
  errorSoft: string;
  info: string;
  controlBg: string;
  controlBgHover: string;
  controlBgPressed: string;
  controlStroke: string;
  controlStrokeHover: string;
  popoverSolid: string;
  focusHalo: string;
}

/// 与 tokens.css 的浅/深两组值一一对应。
const PALETTES: Record<ThemeMode, TsPalette> = {
  light: {
    canvas: "#E3E8EF",
    surfaceGlass: "rgba(255, 255, 255, 0.75)",
    surface: "#FFFFFF",
    // UX01：浮层外壳 = 85% elevated（与 tokens.css --ts-surface-elevated 一致）
    elevated: "rgba(255, 255, 255, 0.85)",
    fill: "rgba(118, 118, 128, 0.12)",
    rowHover: "rgba(118, 118, 128, 0.08)",
    separator: "rgba(0, 0, 0, 0.08)",
    separatorStrong: "rgba(0, 0, 0, 0.14)",
    text: "#1D1D1F",
    textSecondary: "#515154",
    textMuted: "#6E6E73",
    accent: "#0066CC",
    accentFill: "#0071E3",
    accentFillHover: "#0062c4",
    accentFillPressed: "#00539f",
    accentSoft: "rgba(0, 113, 227, 0.1)",
    onAccent: "#FFFFFF",
    success: "#1E7B34",
    warning: "#B25000",
    error: "#D70015",
    errorSoft: "rgba(215, 0, 21, 0.1)",
    info: "#2F6FB0",
    controlBg: "rgba(255, 255, 255, 0.92)",
    controlBgHover: "#ffffff",
    controlBgPressed: "rgba(118, 118, 128, 0.16)",
    controlStroke: "rgba(0, 0, 0, 0.44)",
    controlStrokeHover: "rgba(0, 0, 0, 0.56)",
    popoverSolid: "#ffffff",
    focusHalo: "rgba(0, 113, 227, 0.18)",
  },
  dark: {
    canvas: "#0B0E14",
    surfaceGlass: "rgba(28, 28, 30, 0.75)",
    surface: "#1C1C1E",
    elevated: "rgba(44, 44, 46, 0.85)",
    fill: "rgba(118, 118, 128, 0.24)",
    rowHover: "rgba(118, 118, 128, 0.16)",
    separator: "rgba(255, 255, 255, 0.08)",
    separatorStrong: "rgba(255, 255, 255, 0.16)",
    text: "#F5F5F7",
    textSecondary: "#AEAEB2",
    textMuted: "#8E8E93",
    accent: "#4DA3FF",
    accentFill: "#0060DF",
    accentFillHover: "#0057cc",
    accentFillPressed: "#004bb0",
    accentSoft: "rgba(77, 163, 255, 0.16)",
    onAccent: "#FFFFFF",
    success: "#30D158",
    warning: "#FF9F0A",
    error: "#FF6961",
    errorSoft: "rgba(255, 105, 97, 0.16)",
    info: "#64D2FF",
    controlBg: "rgba(255, 255, 255, 0.07)",
    controlBgHover: "rgba(255, 255, 255, 0.12)",
    controlBgPressed: "rgba(255, 255, 255, 0.04)",
    controlStroke: "rgba(255, 255, 255, 0.34)",
    controlStrokeHover: "rgba(255, 255, 255, 0.46)",
    popoverSolid: "#2c2c2e",
    focusHalo: "rgba(77, 163, 255, 0.24)",
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
  const line = (color: string) => `1px solid ${color}`;
  const inputFocusShadow = `0 0 0 3px ${c.focusHalo}`;
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
      inputColor: c.controlBg,
      actionColor: c.fill,
      hoverColor: c.rowHover,
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
    // UX08：删除无引用的 Card 覆盖——应用不使用 NCard（卡片一律走
    // tokens.css 的 .ts-card 玻璃配方），保留会让人误以为存在第二套卡片样式。
    Button: {
      borderRadiusMedium: "8px",
      borderRadiusSmall: "8px",
      borderRadiusTiny: "8px",
      paddingMedium: "0 16px",
      paddingSmall: "0 12px",
      // 次按钮（default）：焦点相关字段一律等于静止值——鼠标点击不留痕，键盘焦点由 tokens.css 统一画
      color: c.controlBg,
      colorHover: c.controlBgHover,
      colorPressed: c.controlBgPressed,
      colorFocus: c.controlBg,
      colorDisabled: c.controlBg,
      textColor: c.text,
      textColorHover: c.text,
      textColorPressed: c.text,
      textColorFocus: c.text,
      textColorDisabled: c.text,
      border: line(c.controlStroke),
      borderHover: line(c.controlStrokeHover),
      borderPressed: line(c.controlStrokeHover),
      borderFocus: line(c.controlStroke),
      borderDisabled: line(c.controlStroke),
      rippleColor: "#0000",
      // 主按钮：填充色用 accentFill（不是文字强调色 accent）
      colorPrimary: c.accentFill,
      colorHoverPrimary: c.accentFillHover,
      colorPressedPrimary: c.accentFillPressed,
      colorFocusPrimary: c.accentFill,
      colorDisabledPrimary: c.accentFill,
      textColorPrimary: c.onAccent,
      textColorHoverPrimary: c.onAccent,
      textColorPressedPrimary: c.onAccent,
      textColorFocusPrimary: c.onAccent,
      textColorDisabledPrimary: c.onAccent,
      borderPrimary: line(c.accentFill),
      borderHoverPrimary: line(c.accentFillHover),
      borderPressedPrimary: line(c.accentFillPressed),
      borderFocusPrimary: line(c.accentFill),
      borderDisabledPrimary: line(c.accentFill),
      rippleColorPrimary: "#0000",
      // 危险按钮：描边式（次按钮底 + 错误色字与描边）
      colorError: c.controlBg,
      colorHoverError: c.errorSoft,
      colorPressedError: c.errorSoft,
      colorFocusError: c.controlBg,
      colorDisabledError: c.controlBg,
      textColorError: c.error,
      textColorHoverError: c.error,
      textColorPressedError: c.error,
      textColorFocusError: c.error,
      textColorDisabledError: c.error,
      borderError: line(c.error),
      borderHoverError: line(c.error),
      borderPressedError: line(c.error),
      borderFocusError: line(c.error),
      borderDisabledError: line(c.error),
      rippleColorError: "#0000",
      // 文字按钮（text 属性）：强调色文字，各状态不变色
      textColorText: c.accent,
      textColorTextHover: c.accent,
      textColorTextPressed: c.accent,
      textColorTextFocus: c.accent,
      textColorTextDisabled: c.textMuted,
    },
    Input: {
      borderRadius: "8px",
      heightMedium: "32px",
      heightSmall: "28px",
      color: c.controlBg,
      colorFocus: c.controlBg,
      border: line(c.controlStroke),
      borderHover: line(c.controlStrokeHover),
      borderFocus: line(c.accent),
      boxShadowFocus: inputFocusShadow,
    },
    Select: {
      peers: {
        InternalSelection: {
          borderRadius: "8px",
          heightMedium: "32px",
          heightSmall: "28px",
          color: c.controlBg,
          colorActive: c.controlBg,
          border: line(c.controlStroke),
          borderHover: line(c.controlStrokeHover),
          borderActive: line(c.accent),
          borderFocus: line(c.accent),
          boxShadowActive: inputFocusShadow,
          boxShadowFocus: inputFocusShadow,
        },
        InternalSelectMenu: { color: c.popoverSolid },
      },
    },
    Checkbox: {
      color: c.controlBg,
      border: line(c.controlStroke),
      borderChecked: line(c.accentFill),
      colorChecked: c.accentFill,
      borderFocus: line(c.accent),
      boxShadowFocus: inputFocusShadow,
    },
    Switch: {
      railColor: c.controlStroke, // 关闭态轨道本身就是 ≥ 3:1 的灰
      railColorActive: c.accentFill,
      boxShadowFocus: inputFocusShadow,
    },
    TimePicker: { panelColor: c.popoverSolid },
    DatePicker: {
      panelColor: c.popoverSolid,
      peers: { TimePicker: { panelColor: c.popoverSolid } },
    },
    DataTable: {
      // 任务 5：无竖线无外框（bordered=false + single-line 默认），
      // 行间保留 separator 发丝线（DESIGN.md §5 表格）
      borderColor: c.separator,
      borderRadius: "0",
      thColor: c.surface,
      tdColor: c.surface,
      tdColorHover: c.rowHover,
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
