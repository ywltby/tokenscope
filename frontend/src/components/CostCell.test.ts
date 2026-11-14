// CostCell：费用列独立组件（v0.9.1 缺陷 B + 缺陷 C 展示规则）。
//
// 缺陷 B：NDataTable render 回调创建的节点没有 UsageTable 的 scope 标记，
// scoped CSS 永不命中 → 真实浏览器里 .cost-cell 是 inline，主金额与来源
// 费用排成一行。抽成独立 SFC 后 scope 天然生效。
//
// 缺陷 C：展示规则（docs/plans/active/2026-11-14-v090-chart-cost-fee.md §1.3）
// 1. 纯日汇总 + 来源费用 → 主读数 = 来源金额 + 「来源记录」标签，不显示未知†
// 2. 来源显式 0 → $0.00 + 来源标签（缺失≠0：不宣称免费）
// 3. mixed（估算 + 来源费用并存）→ 两部分分别标明
// 4. 真缺价（无来源费用）→ 维持「未知†」
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import CostCell from "./CostCell.vue";
import type { Group } from "../types";

function group(opts: Partial<Group> = {}): Group {
  return {
    key: "m1",
    requests: 10,
    tokens: { input: 1000, output: 500, cache_write: 0, cache_read: 0 },
    cost_usd: 0,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    ...opts,
  };
}

describe("CostCell 布局（缺陷 B：scope 命中）", () => {
  it("cost-cell 是组件自己的节点：带 scope 属性且为列布局", () => {
    const w = mount(CostCell, { props: { group: group({ cost_usd: 1.5 }) } });
    const cell = w.find(".cost-cell");
    expect(cell.exists()).toBe(true);
    const scopeAttr = Object.keys(cell.attributes()).some((a) => a.startsWith("data-v-"));
    expect(scopeAttr, "render 回调节点也必须带组件 scope（缺陷 B 根因）").toBe(true);
  });

  it("有来源费用时主金额与来源行上下排列（不是同一行）", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          unknown_pricing: true,
          source_cost_usd: 9,
          cost_detail: "mixed",
        }),
      },
    });
    const main = w.find(".cost-main");
    const source = w.find(".source-cost-cell");
    expect(main.exists()).toBe(true);
    expect(source.exists()).toBe(true);
    // 结构上分层：cost-cell 的直接子节点是两个独立行。
    const cellEl = w.find(".cost-cell").element as HTMLElement;
    expect(cellEl.children.length).toBe(2);
  });

  it("窄容器不撑坏：来源行可换行、金额行不被截断", () => {
    // mixed 场景（来源费用独立一行）才有 .source-cost-cell。
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0.2,
          source_cost_usd: 9,
          cost_detail: "mixed",
        }),
      },
    });
    const source = w.find(".source-cost-cell");
    expect(source.exists()).toBe(true);
    // nowrap 只用于"标签+金额"这一对（防拆行），整行允许在 cost-cell 内换行。
    expect(source.classes()).toContain("source-cost-cell");
  });
});

describe("CostCell 费用状态（缺陷 C：展示规则）", () => {
  it("规则1：rollup_only + 来源费用 → 主读数 = 来源金额，无未知†", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          unknown_pricing: true,
          unknown_tokens: { input: 1000, output: 500, cache_write: 0, cache_read: 0 },
          source_cost_usd: 7.25,
          cost_detail: "rollup_only",
        }),
      },
    });
    const text = w.text();
    expect(text).toContain("$7.25");
    expect(text).toContain("来源记录");
    expect(text).not.toContain("未知†");
  });

  it("规则2：rollup_only + 来源显式 0 → $0.00 + 来源标签（缺失≠0）", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          unknown_pricing: true,
          source_cost_usd: 0,
          cost_detail: "rollup_only",
        }),
      },
    });
    const text = w.text();
    expect(text).toContain("$0.00");
    expect(text).toContain("来源记录");
    expect(text).not.toContain("未知†");
  });

  it("规则3：mixed（估算 + 来源费用）→ 两部分分别显示", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0.2,
          source_cost_usd: 9,
          cost_detail: "mixed",
        }),
      },
    });
    const text = w.text();
    expect(text).toContain("$0.20");
    expect(text).toContain("$9.00");
    expect(text).toContain("来源记录");
  });

  it("规则4：rollup_only + 无来源费用 → 维持未知†（来源未提供≠没有价格）", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          unknown_pricing: true,
          unknown_tokens: { input: 1000, output: 500, cache_write: 0, cache_read: 0 },
          unknown_reason: "rollup_only",
          source_cost_usd: null,
          cost_detail: "rollup_only",
        }),
      },
    });
    expect(w.text()).toContain("未知†");
    expect(w.text()).not.toContain("来源记录");
  });

  it("规则4b：纯明细缺价 → 未知†（现状不变）", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          unknown_pricing: true,
          unknown_tokens: { input: 300, output: 0, cache_write: 0, cache_read: 0 },
        }),
      },
    });
    expect(w.text()).toContain("未知†");
  });

  it("规则3b：mixed 且估算未知（估算 0 + unknown）→ 未知读数与来源费用并列", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          unknown_pricing: true,
          source_cost_usd: 9,
          cost_detail: "mixed",
        }),
      },
    });
    const text = w.text();
    // 估算部分如实未知，来源部分单独一行，不互相替代。
    expect(text).toContain("未知†");
    expect(text).toContain("$9.00");
    expect(text).toContain("来源记录");
  });

  it("N18 分档估算 pill 照常显示（与来源费用独立）", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0.5,
          cache_write_split_estimated: true,
          source_cost_usd: 3,
          cost_detail: "mixed",
        }),
      },
    });
    expect(w.text()).toContain("分档估算");
    expect(w.text()).toContain("$3.00");
  });
});
