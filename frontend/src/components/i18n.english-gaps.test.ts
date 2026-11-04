// i18n 漏译定向回归（修复 3，2026-11-04 第二轮）：
//   - 费用明细浮层的**单位说明**与**无障碍名称**必须是英文；
//   - 关键组件在英文界面下不得残留中文 aria-label / title（读屏与提示）；
//   - 政策更新提示由前端资源渲染（后端 detail 是中文常量，仅作触发信号）。
//
// 这里只断言"英文界面没有漏译"，中文界面的既有断言仍在各组件自己的用例里。
import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h: hh } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const passthrough = (name: string) =>
    dc({
      name,
      setup(_, { slots }) {
        return () => hh("div", { class: `stub-${name}` }, slots.default?.());
      },
    });
  // NTooltip 透传渲染：trigger 与内容同时可见，便于断言浮层文本。
  const NTooltipStub = dc({
    name: "NTooltip",
    setup(_, { slots }) {
      return () => hh("div", { class: "tooltip-stub" }, [slots.trigger?.(), slots.default?.()]);
    },
  });
  return {
    ...actual,
    NTooltip: NTooltipStub,
    NCollapse: passthrough("NCollapse"),
    NCollapseItem: passthrough("NCollapseItem"),
  };
});

import CostBreakdownTooltip from "./CostBreakdownTooltip.vue";
import SummaryCards from "./SummaryCards.vue";
import UsageTable from "./UsageTable.vue";
import EventTable from "./EventTable.vue";
import DateRangeSelect from "./DateRangeSelect.vue";
import PricingStatusBanner from "./PricingStatusBanner.vue";
import type { EventCostBreakdown, EventList, Group, SummaryReport } from "../types";
import { DEFAULT_LOCALE, setLocale } from "../lib/i18n";
import { UNIT_PRICE_DENOMINATOR } from "../lib/tokenDisplay";

/** 任何 CJK 字符都算漏译（英文界面不允许出现）。 */
const CJK = /[\u3400-\u4dbf\u4e00-\u9fff]/;

function breakdown(): EventCostBreakdown {
  return {
    cost_usd: 0.012,
    complete: true,
    basis_value: 1000,
    cache_write_split_estimated: false,
    lines: [
      { kind: "input", tokens: 800, unit_price: 3, subtotal: 0.0024, priced: true },
      { kind: "output", tokens: 200, unit_price: 15, subtotal: 0.003, priced: true },
    ],
    matched: {
      raw_key: "glm-5.2",
      channel: "custom",
      source: "external",
      matched_key: "glm-5.2",
      match_mode: "full",
      candidate_count: 1,
      complete_candidate_count: 1,
      incomplete_candidate_count: 0,
      reason: "highest_complete_cost",
    },
  } as unknown as EventCostBreakdown;
}

function totals(): Group {
  return {
    key: "totals",
    requests: 3,
    tokens: { input: 800, output: 200, cache_write: 0, cache_read: 0 },
    cost_usd: 0.012,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  };
}

function summaryReport(): SummaryReport {
  return {
    query_id: "q-en",
    pricing_revision: "rev",
    timezone: "UTC",
    generated_at: "2026-11-04T00:00:00Z",
    sources: [],
    by: "model",
    groups: [totals()],
    totals: totals(),
    warnings: [],
    range: null,
  } as unknown as SummaryReport;
}

function eventList(): EventList {
  return {
    query_id: "q-en",
    total: 1,
    rows: [
      {
        cursor: "c1",
        ts: "2026-11-04 10:00:00",
        agent: "claude",
        model: "glm-5.2",
        project: "/repo/app",
        input: 800,
        output: 200,
        cache_write: 0,
        cache_read: 0,
        cost_usd: 0.012,
        cost_breakdown: breakdown(),
      },
    ],
  } as unknown as EventList;
}

function mountBreakdown() {
  return mount(CostBreakdownTooltip, {
    props: { cost: 0.012, breakdown: breakdown(), open: true, pinned: false },
  });
}

describe("费用明细浮层（英文界面）", () => {
  it("unit_note_and_aria_are_translated：单位说明与可访问名称都用英文", () => {
    setLocale("en-US");
    const w = mountBreakdown();
    const trigger = w.find("button.cost-trigger");
    expect(trigger.exists()).toBe(true);
    const aria = trigger.attributes("aria-label") ?? "";
    expect(aria, `无障碍名称漏译：${aria}`).not.toMatch(CJK);
    expect(aria).toContain("Estimated cost");
    expect(aria).toContain("view calculation");

    const unit = w.find(".bd-unit");
    expect(unit.exists()).toBe(true);
    expect(unit.text()).not.toMatch(CJK);
    expect(unit.text()).toContain(UNIT_PRICE_DENOMINATOR);
    expect(unit.text()).toContain("per million tokens");
    setLocale(DEFAULT_LOCALE);
  });

  it("chinese_ui_keeps_existing_wording：中文界面保持原有文案", () => {
    setLocale("zh-CN");
    const w = mountBreakdown();
    expect(w.find("button.cost-trigger").attributes("aria-label")).toContain("查看计算明细");
    expect(w.find(".bd-unit").text()).toContain("单价量纲");
    expect(w.find(".bd-unit").text()).toContain("每百万 token");
  });
});

describe("英文界面的关键组件不得残留中文 aria-label / title", () => {
  function attrsWithCjk(w: ReturnType<typeof mount>): string[] {
    const found: string[] = [];
    for (const node of w.findAll("[aria-label], [title]")) {
      for (const name of ["aria-label", "title"]) {
        const value = node.attributes(name);
        if (value && CJK.test(value)) found.push(`${name}="${value}"`);
      }
    }
    return found;
  }

  it("summary_usage_and_event_tables：指标卡/聚合表/明细表", () => {
    setLocale("en-US");
    const cards = mount(SummaryCards, { props: { totals: totals() } });
    expect(attrsWithCjk(cards)).toEqual([]);
    const usage = mount(UsageTable, { props: { report: summaryReport() } });
    expect(attrsWithCjk(usage)).toEqual([]);
    const events = mount(EventTable, {
      props: { list: eventList(), filterLabel: "Model: glm-5.2" },
    });
    expect(attrsWithCjk(events)).toEqual([]);
    setLocale(DEFAULT_LOCALE);
  });

  it("filters_and_banners：时间范围与定价横幅", () => {
    setLocale("en-US");
    const range = mount(DateRangeSelect, {
      props: { preset: { kind: "days", days: 7 }, tz: "UTC", range: null },
    });
    expect(attrsWithCjk(range)).toEqual([]);
    const banner = mount(PricingStatusBanner, { global: { stubs: { NCollapse: true } } });
    expect(attrsWithCjk(banner)).toEqual([]);
    setLocale(DEFAULT_LOCALE);
  });

  it("tooltip_trigger：费用明细触发器", () => {
    setLocale("en-US");
    expect(attrsWithCjk(mountBreakdown())).toEqual([]);
    setLocale(DEFAULT_LOCALE);
  });
});

describe("长英文文案不撑破既有排版约束（回归）", () => {
  it("date_range_trigger_keeps_bounded_width：英文范围标签仍在触发器宽度上限内", async () => {
    setLocale("en-US");
    const w = mount(DateRangeSelect, {
      props: {
        preset: { kind: "custom", start: "2024-01-01T00:00", end: "now" },
        tz: "UTC",
        range: {
          granularity: "instant",
          start_local: "2024-01-01T00:00",
          end_local: "2026-11-04T23:59",
          from: null,
          to: null,
        },
      } as never,
    });
    await flushPromises();
    // 触发器仍然只渲染一个语义标签（不新增第二个可见文本节点）
    expect(w.findAll(".range-trigger-label")).toHaveLength(1);
    setLocale(DEFAULT_LOCALE);
  });
});
