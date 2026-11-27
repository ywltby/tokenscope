import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AXIS_LABEL_MARGIN,
  GRID_LEFT_OUTER,
  LABEL_WIDTH_SAFETY_PAD,
  clearLabelMeasureCache,
  computeCategoryLabelLayout,
  computeMaxLabelBudget,
  truncateLabelEnd,
  visibleCategoryRange,
} from "./chartLabelLayout";

describe("chartLabelLayout（全量测量与末尾省略）", () => {
  beforeEach(() => {
    clearLabelMeasureCache();
  });

  it("43 行长短混排时逐一测量全部可见标签，绝不跳过奇数行长名称", () => {
    const categories = Array.from({ length: 43 }, (_, i) => {
      if (i === 1) return "DeepSeek V4.1 Flash 1017";
      if (i === 19) return "DeepSeek V4 Flash 0731";
      if (i === 41) return "Nemotron 3 Ultra 550B A55B";
      return `m${String(i).padStart(2, "0")}`;
    });
    const seen = new Set<string>();
    const measure = vi.fn((text: string) => {
      seen.add(text);
      return text.length * 7;
    });

    const layout = computeCategoryLabelLayout(
      categories,
      { start: 0, end: 100 },
      1070,
      "sans-serif",
      measure,
    );

    expect(layout.range).toEqual({ startIndex: 0, endIndex: 42, visibleCount: 43 });
    expect(layout.height).toBe(43 * 24 + 80);
    // 所有 43 行都参与测量（包括旧抽样算法会跳过的奇数行 1、19、41）。
    for (const name of categories) {
      expect(seen.has(name), `漏测标签: ${name}`).toBe(true);
    }
    const longest = "Nemotron 3 Ultra 550B A55B";
    const expectedWidth = longest.length * 7;
    expect(layout.maxVisibleWidth).toBe(expectedWidth);
    expect(layout.gridLeft).toBe(
      GRID_LEFT_OUTER + Math.ceil(expectedWidth) + LABEL_WIDTH_SAFETY_PAD + AXIS_LABEL_MARGIN,
    );
    // 有足够空间时显示全名，不省略。
    expect(layout.formatter(longest)).toBe(longest);
  });

  it("空间不足时保留开头、末尾加省略号，不从左边裁切字符", () => {
    const measure = (text: string) => text.length * 8;
    const longName = "Project 00 / a-very-long-project-directory-name-for-truncation";
    const res = truncateLabelEnd(longName, 160, "12px sans-serif", measure);
    expect(res.truncated).toBe(true);
    expect(res.text.startsWith("Project 00 / ")).toBe(true);
    expect(res.text.endsWith("…")).toBe(true);
    expect(res.width).toBeLessThanOrEqual(160);

    // 空间充足时完整显示。
    const shortName = "DeepSeek V4.1 Flash 1017";
    const full = truncateLabelEnd(shortName, 240, "12px sans-serif", measure);
    expect(full.truncated).toBe(false);
    expect(full.text).toBe(shortName);
  });

  it("缩放或平移窗口后只按当前可见范围重算左侧预留空间", () => {
    const categories = Array.from({ length: 43 }, (_, i) =>
      i === 41 ? "Nemotron 3 Ultra 550B A55B" : `m${String(i).padStart(2, "0")}`,
    );
    const measure = (text: string) => text.length * 7;

    // 仅看前 20 行（全是短名）→ 左侧预留收窄，把宽度还给绘图区。
    const shortWindow = computeCategoryLabelLayout(
      categories,
      { start: 0, end: 45, startValue: 0, endValue: 19 },
      1070,
      "sans-serif",
      measure,
    );
    // 平移到后 20 行（同一行数、但包含索引 41 的长名）→ 左侧预留自动扩大。
    const longWindow = computeCategoryLabelLayout(
      categories,
      { start: 55, end: 100, startValue: 23, endValue: 42 },
      1070,
      "sans-serif",
      measure,
    );
    expect(shortWindow.height).toBe(longWindow.height);
    expect(longWindow.gridLeft).toBeGreaterThan(shortWindow.gridLeft + 100);
  });

  it("value 模式数据变少时按平移后的有效边界计数", () => {
    expect(visibleCategoryRange(20, { start: 10, end: 60, startValue: 4, endValue: 23 })).toEqual({
      startIndex: 0,
      endIndex: 19,
      visibleCount: 20,
    });
  });

  it("窄窗口预算不超过容器可用空间且始终保留绘图区", () => {
    expect(computeMaxLabelBudget(1070)).toBe(260);
    expect(computeMaxLabelBudget(874)).toBe(244);
    expect(computeMaxLabelBudget(420)).toBe(116);
    expect(computeMaxLabelBudget(320)).toBe(80);
  });
});
