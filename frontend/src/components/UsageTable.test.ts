// 计划 C5：费用未知的追溯完整（v0.9.3 起不再有独立未知列与 † 脚注）——
// 未定价 token 明细进费用浮层，金额用警告色表达；全部已计价时用普通文字色。
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import UsageTable from "./UsageTable.vue";
// Vite ?raw：直接取 SFC 源文本做样式契约断言（项目无 @types/node）
import type { Group, SummaryReport } from "../types";

function group(key: string, unknownTokens?: Partial<Group["unknown_tokens"]>): Group {
  return {
    key,
    requests: 1,
    tokens: { input: 10, output: 5, cache_write: 0, cache_read: 0 },
    cost_usd: 0.01,
    unknown_pricing: unknownTokens !== undefined,
    unknown_tokens: {
      input: unknownTokens?.input ?? 0,
      output: unknownTokens?.output ?? 0,
      cache_write: unknownTokens?.cache_write ?? 0,
      cache_read: unknownTokens?.cache_read ?? 0,
    },
  };
}

function report(groups: Group[], totalsUnknown: boolean): SummaryReport {
  return {
    query_id: "q",
    pricing_revision: "rev",
    timezone: "Asia/Shanghai",
    generated_at: "t",
    sources: [],
    by: "model",
    groups,
    totals: {
      ...group("合计"),
      unknown_pricing: totalsUnknown,
    },
    warnings: [],
  };
}

describe("UsageTable 费用列单金额（v0.9.2）", () => {
  function costMain(group: Group, totalsUnknown = false): string {
    const w = mount(UsageTable, {
      props: { report: report([{ ...group }], totalsUnknown) },
    });
    return w.find(".gcb-amount-main").text();
  }
  function costTone(group: Group, totalsUnknown = false): string[] {
    const w = mount(UsageTable, {
      props: { report: report([{ ...group }], totalsUnknown) },
    });
    return w.find(".gcb-amount-main").classes();
  }

  it("明细估算 $1.00 + 来源费用 $7.25 → 行内只显示合并金额 $8.25", () => {
    const g = group("m-a", { input: 100 });
    g.cost_usd = 1.0;
    g.source_cost_usd = 7.25;
    g.total_cost_usd = 8.25;
    g.cost_detail = "mixed";
    const main = costMain(g, false);
    expect(main).toContain("$8.25");
    expect(main).not.toContain("来源记录");
    expect(main).not.toContain("$7.25");
    // 悬浮详情在表格里也存在（挂 body），常驻区不含来源长文。
    const r = report([{ ...g }], false);
    const w = mount(UsageTable, { props: { report: r } });
    expect(w.find(".gcb-trigger").exists()).toBe(true);
  });

  it("纯汇总组：主读数 = 来源费用单金额", () => {
    const g = group("m-b", { input: 50 });
    g.cost_usd = 0;
    g.source_cost_usd = 7.25;
    g.total_cost_usd = 7.25;
    g.cost_detail = "rollup_only";
    expect(costMain(g, false)).toContain("$7.25");
  });

  it("部分未定价 → 橙色金额，无脚注符号（无来源长文）", () => {
    const g = group("m-c", { input: 100 });
    g.cost_usd = 0.5;
    g.total_cost_usd = 0.5;
    g.cost_partially_unknown = true;
    g.unknown_pricing = true;
    g.unknown_tokens = { input: 300, output: 0, cache_write: 0, cache_read: 0 };
    const main = costMain(g, true);
    expect(main).toContain("$0.50");
    expect(main).not.toContain("†");
    expect(costTone(g, true)).toContain("gcb-warning");
    expect(main).not.toContain("来源");
  });

  it("完全没有费用依据 → 「未知」警告色", () => {
    const g = group("m-d", { input: 100 });
    g.cost_usd = 0;
    g.total_cost_usd = null;
    g.unknown_pricing = true;
    g.unknown_tokens = { input: 100, output: 0, cache_write: 0, cache_read: 0 };
    expect(costMain(g, true)).toContain("未知");
    expect(costMain(g, true)).not.toContain("未知†");
    expect(costTone(g, true)).toContain("gcb-warning");
  });

  it("全部已计价时不出现未知列与脚注，金额用普通文字色", () => {
    const g = group("m-priced");
    g.cost_usd = 0.01;
    g.total_cost_usd = 0.01;
    const main = costMain(g, false);
    expect(main).toContain("$0.01");
    expect(main).not.toContain("†");
    expect(costTone(g, false)).not.toContain("gcb-warning");
    const w = mount(UsageTable, { props: { report: report([{ ...g }], false) } });
    expect(
      (w.vm as unknown as { columns: { key?: string }[] }).columns.some(
        (c) => c.key === "unknown_tokens",
      ),
    ).toBe(false);
  });

  it("费用节点是组件挂载的按钮（缺陷 B 结构回归锚点）", () => {
    // v0.9.2：费用列不再依赖 scoped CSS（全局 gcb- 前缀 + 组件化），render
    // 直建 DOM 的粘行问题结构上不可能复发；断言更新为组件挂载结构验证。
    const g = group("m-scope");
    g.cost_usd = 1.0;
    g.total_cost_usd = 1.0;
    const w = mount(UsageTable, { props: { report: report([g], false) } });
    const trigger = w.find(".gcb-trigger");
    expect(trigger.exists()).toBe(true);
    expect(trigger.element.tagName).toBe("BUTTON");
    expect(trigger.attributes("aria-label")).toContain("$1.00");
  });

  it("合计行与模型行同一单金额规则", () => {
    const g = group("m-a", { input: 100 });
    g.cost_usd = 1.0;
    g.source_cost_usd = 7.25;
    g.total_cost_usd = 8.25;
    g.cost_detail = "mixed";
    const r = report([g], false);
    r.totals.total_cost_usd = 8.25;
    r.groups = [...r.groups, { ...r.totals }];
    const w = mount(UsageTable, { props: { report: r } });
    const mains = w.findAll(".gcb-amount-main");
    expect(mains.length).toBeGreaterThanOrEqual(2);
    for (const m of mains) {
      expect(m.text()).toContain("$8.25");
      expect(m.text()).not.toContain("来源记录");
    }
  });

  it("日粒度/来源大段说明不再常驻（挪进悬浮详情）", () => {
    const g = group("m-a", { input: 100 });
    g.cost_usd = 0;
    g.source_cost_usd = 7.25;
    g.total_cost_usd = 7.25;
    g.cost_detail = "rollup_only";
    g.unknown_reason = "rollup_only";
    const r = report([g], false);
    r.totals.unknown_reason = "rollup_only";
    const w = mount(UsageTable, { props: { report: r } });
    const text = w.text();
    expect(text).not.toContain("CCS 来源记录费用");
    expect(text).not.toContain("其中 $7.25 来自");
  });
});

// N18/T3 披露进悬浮：费用列不再常驻分档估算/按 0 估算/未计价胶囊。
describe("UsageTable N18/T3 披露进悬浮（v0.9.2）", () => {
  function costMain(group: Group, totalsUnknown = false): string {
    const w = mount(UsageTable, {
      props: { report: report([{ ...group }], totalsUnknown) },
    });
    return w.find(".gcb-amount-main").text();
  }

  it("分档估算组：主金额，无常驻胶囊", () => {
    const g = group("g-est", { input: 10 });
    g.cost_usd = 7.5;
    g.total_cost_usd = 7.5;
    g.cache_write_split_estimated = true;
    g.cost_partially_unknown = false;
    const main = costMain(g, false);
    expect(main).toContain("$7.50");
    expect(main).not.toContain("分档估算");
    expect(main).not.toContain("含未计价");
  });

  it("真正未计价组仍显示「未知」，不冒充分档估算", () => {
    const g = group("g-unpriced", { input: 10 });
    g.cost_usd = 0;
    g.total_cost_usd = null;
    g.unknown_pricing = true;
    g.unknown_tokens = { input: 2_000_000, output: 0, cache_write: 0, cache_read: 0 };
    const main = costMain(g, true);
    expect(main).toContain("未知");
    expect(main).not.toContain("†");
    expect(main).not.toContain("分档估算");
  });

  it("混合组（估算 + 未计价）：橙色金额、无脚注符号", () => {
    const g = group("g-mixed", { input: 10 });
    g.cost_usd = 3.75;
    g.total_cost_usd = 3.75;
    g.unknown_pricing = true;
    g.cost_partially_unknown = true;
    g.unknown_tokens = { input: 0, output: 0, cache_write: 0, cache_read: 1_000_000 };
    const main = costMain(g, true);
    expect(main).toContain("$3.75");
    expect(main).not.toContain("†");
  });

  it("按 0 估算组：金额入主读数、普通文字色（unknown=0）", () => {
    const g = group("g-zero");
    g.cost_usd = 0;
    g.total_cost_usd = 0;
    g.cache_price_zero_estimated = true;
    g.unknown_pricing = false;
    g.unknown_tokens = { input: 0, output: 0, cache_write: 0, cache_read: 0 };
    const main = costMain(g, false);
    expect(main).toContain("$0.00");
    expect(main).not.toContain("按零估算");
    const w = mount(UsageTable, { props: { report: report([{ ...g }], false) } });
    expect(w.find(".gcb-amount-main").classes()).not.toContain("gcb-warning");
  });
});

// PR #72 回归：金额按钮的 Enter/Space/点击只操作费用浮层，不冒泡触发行下钻。
// 反例：日期行金额上按 Enter，行处理器 preventDefault 吞掉原生激活并下钻，
// 浮层没有固定、明细被加上 day 筛选。行本身聚焦时 Enter/Space 仍须下钻。
describe("费用按钮与表格行操作隔离（PR #72 回归）", () => {
  function mountTable() {
    const g = group("2026-09-01");
    g.cost_usd = 0.01;
    g.total_cost_usd = 0.01;
    const r = report([g], false);
    r.by = "day";
    r.groups = [...r.groups, { ...r.totals }];
    return mount(UsageTable, { props: { report: r }, attachTo: document.body });
  }

  it("金额按钮 Enter：不吞原生激活、不下钻，一次 click 只固定一次", async () => {
    const w = mountTable();
    const row = w.find("tbody tr:not(.total-row)");
    const btn = row.find(".gcb-trigger");
    const keydown = new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
    });
    btn.element.dispatchEvent(keydown);
    await nextTick();
    expect(keydown.defaultPrevented).toBe(false);
    expect(w.emitted("row-click")).toBeUndefined();
    // 原生 button 的 Enter 等价于一次 click：只固定，不二次切换、不下钻。
    btn.element.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    await nextTick();
    expect(btn.attributes("aria-describedby")).toBeTruthy();
    expect(w.emitted("row-click")).toBeUndefined();
    w.unmount();
  });

  it("金额按钮 Space：不吞原生激活、不下钻", async () => {
    const w = mountTable();
    const btn = w.find("tbody tr:not(.total-row) .gcb-trigger");
    const keydown = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
    btn.element.dispatchEvent(keydown);
    await nextTick();
    expect(keydown.defaultPrevented).toBe(false);
    expect(w.emitted("row-click")).toBeUndefined();
    w.unmount();
  });

  it("金额按钮鼠标点击：只切换固定态，一次点击不触发两次切换、不下钻", async () => {
    const w = mountTable();
    const btn = w.find("tbody tr:not(.total-row) .gcb-trigger");
    btn.element.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    await nextTick();
    expect(btn.attributes("aria-describedby")).toBeTruthy();
    expect(w.emitted("row-click")).toBeUndefined();
    btn.element.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    await nextTick();
    expect(btn.attributes("aria-describedby")).toBeUndefined();
    expect(w.emitted("row-click")).toBeUndefined();
    w.unmount();
  });

  it("行本身聚焦时 Enter/Space 仍下钻；点击行内非按钮区域仍下钻", async () => {
    const w = mountTable();
    const row = w.find("tbody tr:not(.total-row)");
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    row.element.dispatchEvent(enter);
    await nextTick();
    expect(enter.defaultPrevented).toBe(true);
    expect(w.emitted("row-click")).toEqual([["2026-09-01"]]);
    const space = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
    row.element.dispatchEvent(space);
    await nextTick();
    expect(space.defaultPrevented).toBe(true);
    expect(w.emitted("row-click")?.length).toBe(2);
    row.find(".dim-cell").element.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await nextTick();
    expect(w.emitted("row-click")?.length).toBe(3);
    w.unmount();
  });
});
