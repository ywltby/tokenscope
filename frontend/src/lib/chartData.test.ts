// 计划 A4（F03）：chart_labels_match_series——名称序与用量序相反时，
// 每个分类标签仍必须对齐自己的数值（修复前：标签重排而 series 保持
// 原始顺序，标签与数值错位）。
import { describe, expect, it } from "vitest";
import {
  SERIES,
  buildBarChartData,
  chartStateText,
  chartSummaryLines,
  fullLabels,
  realGroups,
} from "./chartData";
import type { Group } from "../types";

function group(key: string, input: number, output = 0): Group {
  return {
    key,
    requests: 1,
    tokens: { input, output, cache_write: 0, cache_read: 0 },
    cost_usd: 0,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  };
}

describe("buildBarChartData", () => {
  it("chart_labels_match_series：非日维度按用量降序且每个标签对齐自己的数值", () => {
    // 名称序 A,B；用量序 B(100),A(10)——故意相反。
    const data = buildBarChartData([group("A", 10), group("B", 100)], "project");
    expect(data.categories).toEqual(["B", "A"]);
    expect(data.series.map((s) => s.name)).toEqual(["输入", "输出", "缓存写", "缓存命中"]);
    const input = data.series[0].values;
    expect(input).toEqual([100, 10]); // 与 categories 同源 → 不错位
    // 逐桶断言：每个标签下的每个分项都取自同一个 group。
    const byKey: Record<string, Group["tokens"]> = {
      A: { input: 10, output: 0, cache_write: 0, cache_read: 0 },
      B: { input: 100, output: 0, cache_write: 0, cache_read: 0 },
    };
    data.categories.forEach((key, i) => {
      data.series.forEach((s) => {
        // series.name 是图例标签，经 SERIES 还原为 token 键再对账。
        const tokenKey = SERIES.find((x) => x.label === s.name)!.name;
        expect(s.values[i]).toBe(byKey[key][tokenKey]);
      });
    });
  });

  it("chart_labels_match_series：日维度保持时间序，不做用量重排", () => {
    const data = buildBarChartData([group("2026-10-01", 1), group("2026-10-02", 999)], "day");
    expect(data.categories).toEqual(["2026-10-01", "2026-10-02"]);
    expect(data.series[0].values).toEqual([1, 999]);
  });

  it("平局时保持稳定顺序（同用量不重排）", () => {
    const data = buildBarChartData([group("x", 5), group("y", 5)], "model");
    expect(data.categories).toEqual(["x", "y"]);
    expect(data.series[0].values).toEqual([5, 5]);
  });
});

const g = (key: string, input: number, output = 0, cw = 0, cr = 0): Group => ({
  key,
  label: key.split("/").pop() ?? key,
  requests: 1,
  tokens: { input, output, cache_write: cw, cache_read: cr },
  cost_usd: 0,
  unknown_pricing: false,
  unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
});

describe("chartData 显示元数据（设计系统 Task 4）", () => {
  it("fullLabels 与分类顺序一致，携带完整原始键（长名 tooltip 用）", () => {
    const groups = [g("C:/work/very-long-project-name/alpha-service", 10), g("short", 5)];
    const { categories } = buildBarChartData(groups, "project");
    const full = fullLabels(groups, "project");
    expect(full).toHaveLength(categories.length);
    expect(full[0]).toBe("C:/work/very-long-project-name/alpha-service");
    expect(categories[0]).toBe("alpha-service");
  });

  it("文字摘要与图表数据等价（逐类别四类 token + 合计）", () => {
    const groups = [g("a", 100, 20, 5, 8), g("b", 50)];
    const { categories, series } = buildBarChartData(groups, "model");
    const lines = chartSummaryLines(groups, "model");
    expect(lines[0]).toContain("模型");
    for (let i = 0; i < categories.length; i++) {
      const line = lines[i + 1];
      expect(line).toContain(categories[i]);
      for (const s of series) expect(line).toContain(String(s.values[i]));
      const total = series.reduce((acc, s) => acc + s.values[i], 0);
      expect(line).toContain(`合计 ${total}`);
    }
  });

  it("空数据、单类别、多类别有明确状态文案", () => {
    expect(chartStateText(0)).toContain("暂无数据");
    expect(chartStateText(1)).toContain("1 个类别");
    expect(chartStateText(50)).toContain("50 个类别");
    // 多类别必须说明可滚动查看
    expect(chartStateText(50)).toContain("滚动");
  });
});

describe("realGroups（UX05：合计不是可绘制类别）", () => {
  it("排除合计行，保留真实类别顺序", () => {
    const groups = [g("2026-10-01", 1), g("2026-10-02", 2), g("合计", 3)];
    expect(realGroups(groups).map((x) => x.key)).toEqual(["2026-10-01", "2026-10-02"]);
  });

  it("仅有合计时返回空数组（父级据此显示明确空状态）", () => {
    expect(realGroups([g("合计", 3)])).toHaveLength(0);
  });

  it("无合计行时原样返回", () => {
    const groups = [g("a", 1), g("b", 2)];
    expect(realGroups(groups)).toHaveLength(2);
  });
});
