// UX01：跨 CSS / Naive / ECharts 的语义 token 一致性契约。
//
// 不机械比较所有色值字符串，而是按**显式字段映射**比较两套主题（浅/深）中
// 每个语义槽位：先解析 tokens.css 的 :root 与 :root[data-theme="dark"] 作用域
// （不是全文件首次 regex 命中），再与 naiveThemeOverrides / chartTokens 的
// 消费值逐一对照；颜色做 hex/rgba 归一化（大小写、空白）。
import { describe, expect, it } from "vitest";
// Vite ?raw：直接取 tokens.css 源文本（项目无 @types/node，不用 node:fs）
import tokensCss from "./tokens.css?raw";
import { naiveThemeOverrides } from "./naiveTheme";
import { chartTokens } from "./chartTheme";
import type { ThemeMode } from "../composables/theme";

const css = tokensCss.replace(/\/\*[\s\S]*?\*\//g, "");

/// 按作用域取出规则体（花括号配对），避免匹配到别的主题/降级分支。
/// selector 不含尾随 `{`（如 `:root` / `:root[data-theme="dark"]`）。
function scopedBody(selector: string): string {
  const idx = css.indexOf(selector);
  if (idx === -1) throw new Error(`tokens.css 未找到作用域 ${selector}`);
  const open = css.indexOf("{", idx + selector.length);
  let depth = 0;
  for (let i = open; i < css.length; i++) {
    if (css[i] === "{") depth++;
    else if (css[i] === "}") {
      depth--;
      if (depth === 0) return css.slice(open + 1, i);
    }
  }
  throw new Error(`作用域 ${selector} 未闭合`);
}

function parseVars(body: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)) out[m[1]] = m[2].trim();
  return out;
}

/// 普通声明（非自定义属性）解析，如 font-size / line-height。
function parseDecls(body: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/(^|[;\s])([a-z-]+)\s*:\s*([^;]+);/g)) out[m[2]] = m[3].trim();
  return out;
}

const TOKENS: Record<ThemeMode, Record<string, string>> = {
  light: parseVars(scopedBody(":root")),
  dark: parseVars(scopedBody(':root[data-theme="dark"]')),
};

const norm = (v: string): string => v.trim().toLowerCase().replace(/\s+/g, "");

type Overrides = Record<string, Record<string, string>>;
const naive = (mode: ThemeMode): Overrides => naiveThemeOverrides(mode) as unknown as Overrides;

// 显式字段映射：CSS token → Naive 消费槽位。
const NAIVE_MAP: [string, (o: Overrides) => string | undefined][] = [
  ["--ts-text", (o) => o.common.textColorBase],
  ["--ts-text-secondary", (o) => o.common.textColor2],
  ["--ts-text-muted", (o) => o.common.textColor3],
  ["--ts-separator", (o) => o.common.borderColor],
  ["--ts-surface-solid", (o) => o.DataTable.tdColor],
  ["--ts-surface-solid", (o) => o.DataTable.thColor],
  ["--ts-text-secondary", (o) => o.DataTable.thTextColor],
  ["--ts-text", (o) => o.DataTable.tdTextColor],
  ["--ts-surface-elevated", (o) => o.Tooltip.color],
  ["--ts-surface-elevated", (o) => o.Popover.color],
  ["--ts-text", (o) => o.Tooltip.textColor],
  ["--ts-accent", (o) => o.common.primaryColor],
  ["--ts-error", (o) => o.common.errorColor],
  ["--ts-warning", (o) => o.common.warningColor],
  ["--ts-success", (o) => o.common.successColor],
];

describe("themeContract：适配器与 CSS 语义 token 对齐（UX01）", () => {
  it("adapters_match_semantic_tokens_in_both_themes", () => {
    for (const mode of ["light", "dark"] as ThemeMode[]) {
      const o = naive(mode);
      for (const [token, pick] of NAIVE_MAP) {
        const cssValue = TOKENS[mode][token];
        expect(cssValue, `${mode} tokens.css 缺少 ${token}`).toBeTruthy();
        expect(norm(pick(o) ?? ""), `${mode} ${token} → Naive`).toBe(norm(cssValue));
      }
    }
  });

  it("chart_adapters_match_semantic_tokens_in_both_themes", () => {
    const CHART_MAP: [string, (t: ReturnType<typeof chartTokens>) => string][] = [
      ["--ts-text", (t) => t.text],
      ["--ts-text-secondary", (t) => t.textSecondary],
      ["--ts-text-muted", (t) => t.textMuted],
      ["--ts-separator", (t) => t.separator],
      // DESIGN.md §5：网格线用 --ts-separator
      ["--ts-separator", (t) => t.splitLine],
      // DESIGN.md §5：tooltip 用 elevated 浮层
      ["--ts-surface-elevated", (t) => t.tooltipBg],
      ["--ts-chart-input", (t) => t.series[0].color],
      ["--ts-chart-output", (t) => t.series[1].color],
      ["--ts-chart-cache-write", (t) => t.series[2].color],
      ["--ts-chart-cache-read", (t) => t.series[3].color],
    ];
    for (const mode of ["light", "dark"] as ThemeMode[]) {
      const t = chartTokens(mode);
      for (const [token, pick] of CHART_MAP) {
        expect(norm(pick(t)), `${mode} ${token} → ECharts`).toBe(norm(TOKENS[mode][token]));
      }
    }
  });

  it("软状态背景在两主题都有独立 token（D1，不再散落 rgba 字面量）", () => {
    for (const mode of ["light", "dark"] as ThemeMode[]) {
      for (const key of ["--ts-success-soft", "--ts-warning-soft", "--ts-error-soft"]) {
        expect(TOKENS[mode][key], `${mode} 缺少 ${key}`).toMatch(/^rgba?\(/);
      }
    }
    // 浅深两主题取值不同（各自维护）
    expect(norm(TOKENS.light["--ts-warning-soft"])).not.toBe(
      norm(TOKENS.dark["--ts-warning-soft"]),
    );
  });

  it("table_typography_is_scoped_to_datatable：表格排版只作用于 DataTable", () => {
    for (const mode of ["light", "dark"] as ThemeMode[]) {
      const o = naive(mode);
      // 正文 13px/1.45/text，表头 12px/500/secondary
      expect(o.DataTable.fontSizeSmall).toBe("13px");
      expect(o.DataTable.lineHeight).toBe("1.45");
      expect(o.DataTable.thFontWeight).toBe("500");
      // 不改 common.fontSizeSmall（否则影响全站 small 控件）
      expect(o.common.fontSizeSmall).toBeUndefined();
      expect(o.common.heightSmall).toBe("28px");
      expect(o.common.heightMedium).toBe("32px");
    }
    // 表头 12px 由 DataTable 作用域 CSS 覆盖（主题只有单一 fontSize）
    const th = parseDecls(scopedBody(".n-data-table th"));
    expect(th["font-size"]).toBe("12px");
    expect(th["line-height"]).toBe("1.4");
    expect(th["font-weight"]).toBe("500");
    const td = parseDecls(scopedBody(".n-data-table td"));
    expect(td["font-size"]).toBe("13px");
    expect(td["line-height"]).toBe("1.45");
  });

  it("浮层材质契约：85% elevated + 16px 模糊（Naive 与 ECharts 同配方）", () => {
    for (const mode of ["light", "dark"] as ThemeMode[]) {
      const alpha = /rgba\([^)]*,\s*([\d.]+)\)/.exec(naive(mode).Popover.color)?.[1];
      expect(Number(alpha), `${mode} 浮层应为 85% elevated`).toBeCloseTo(0.85, 3);
    }
    // 浮层模糊配方 16px（Naive CSS 与 ECharts extraCssText 同源）
    expect(norm(TOKENS.light["--ts-glass-blur-popover"])).toBe("blur(16px)saturate(150%)");
  });
});
