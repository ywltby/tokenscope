// 计划 A4（F03）：chart_labels_match_series——名称序与用量序相反时，
// 每个分类标签仍必须对齐自己的数值（修复前：标签重排而 series 保持
// 原始顺序，标签与数值错位）。
import { describe, expect, it } from "vitest";
import { SERIES, buildBarChartData } from "./chartData";
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
    expect(data.series.map((s) => s.name)).toEqual(["输入", "输出", "缓存写", "缓存读"]);
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
