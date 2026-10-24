// U04：筛选偏好（localStorage `tokenscope-view-filters`）。
// 偏好决定"查什么"：来源、维度、时间预设、日粒度参与方式、下钻。
// 非法/半非法值必须逐字段回退默认（全部来源、按时间、全部时间、自动参与、无下钻），
// 绝不把脏值带进查询参数。
import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_VIEW_FILTERS,
  VIEW_FILTERS_KEY,
  loadViewFilters,
  normalizeViewFilters,
  saveViewFilters,
  viewFiltersEqual,
} from "./viewFilters";

beforeEach(() => {
  localStorage.clear();
});

describe("筛选偏好校验与持久化（U04）", () => {
  it("defaults_when_missing_or_broken：缺失、非法 JSON、半非法对象都回退默认", () => {
    expect(loadViewFilters()).toEqual(DEFAULT_VIEW_FILTERS);

    localStorage.setItem(VIEW_FILTERS_KEY, "{oops");
    expect(loadViewFilters()).toEqual(DEFAULT_VIEW_FILTERS);

    localStorage.setItem(
      VIEW_FILTERS_KEY,
      JSON.stringify({
        by: "nope",
        agent: "nope",
        preset: { kind: "days", days: 3 },
        rollupsMode: "nope",
        drill: { type: "nope", key: "" },
      }),
    );
    expect(loadViewFilters()).toEqual(DEFAULT_VIEW_FILTERS);
  });

  it("valid_values_round_trip：合法值原样往返，含自定义预设与下钻", () => {
    const filters = {
      by: "model" as const,
      agent: "claude" as const,
      preset: { kind: "days", days: 7 } as const,
      rollupsMode: "detail_only" as const,
      drill: { type: "project" as const, key: "C:/work/alpha", label: "alpha" },
    };
    saveViewFilters(filters);
    expect(loadViewFilters()).toEqual(filters);
  });

  it("invalid_drill_is_dropped_but_other_fields_kept：下钻非法只清下钻，其余偏好保留", () => {
    const normalized = normalizeViewFilters({
      by: "project",
      agent: "codex",
      preset: { kind: "rolling", hours: 24 },
      rollupsMode: "detail_only",
      drill: { type: "model", key: "" },
    });
    expect(normalized).toEqual({
      by: "project",
      agent: "codex",
      preset: { kind: "rolling", hours: 24 },
      rollupsMode: "detail_only",
      drill: null,
    });
  });

  it("equality_compares_preset_and_drill：相等判定逐项比较（预设按身份键、下钻按类型与键）", () => {
    const a = { ...DEFAULT_VIEW_FILTERS, preset: { kind: "days", days: 1 } as const };
    const b = { ...DEFAULT_VIEW_FILTERS, preset: { kind: "days", days: 1 } as const };
    expect(viewFiltersEqual(a, b)).toBe(true);
    expect(viewFiltersEqual(a, { ...b, preset: { kind: "days", days: 7 } })).toBe(false);
    expect(viewFiltersEqual(a, { ...b, preset: null })).toBe(false);
    const withDrill = { ...b, drill: { type: "day" as const, key: "2026-10-01" } };
    expect(viewFiltersEqual(b, withDrill)).toBe(false);
    expect(viewFiltersEqual(withDrill, { ...withDrill, drill: { ...withDrill.drill } })).toBe(true);
  });
});

// WB06：四应用的筛选归一化——新增合法值通过、未知值回退全部。
describe("WB06 来源筛选四应用", () => {
  it("workbuddy / workbuddy-ai 是合法筛选值并被保留", () => {
    for (const agent of ["workbuddy", "workbuddy-ai"] as const) {
      const f = normalizeViewFilters({ agent });
      expect(f.agent).toBe(agent);
    }
  });
  it("未知应用回退 all（视图快照旧值安全）", () => {
    expect(normalizeViewFilters({ agent: "gpt" }).agent).toBe("all");
  });
});
