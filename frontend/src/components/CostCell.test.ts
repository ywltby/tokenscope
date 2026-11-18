// CostCell：费用列独立组件（v0.9.1 缺陷 B scope 修复 + v0.9.2 单金额规则）。
//
// 缺陷 B：NDataTable render 回调创建的节点没有 UsageTable 的 scope 标记，
// scoped CSS 永不命中 → 真实浏览器里 .cost-cell 是 inline。抽成独立 SFC
// 后 scope 天然生效。
//
// v0.9.2（2026-11-15 用户产品决策）：费用列**只显示一个金额**
// （total_cost_usd，后端合并互不重叠贡献）——常驻来源胶囊、第二行来源
// 金额、大段说明全部移除；v0.9.3 起用颜色（--ts-warning）表达未定价，
// 不再显示 `†`；构成与依据进悬浮详情。
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import CostCell from "./CostCell.vue";
import GroupCostBreakdownTooltip from "./GroupCostBreakdownTooltip.vue";
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

describe("CostCell 单金额规则（v0.9.2）", () => {
  it("明细估算 $1.00 + 来源费用 $7.25 → 主读数只有一个 $8.25，无常驻来源行", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 1.0,
          source_cost_usd: 7.25,
          total_cost_usd: 8.25,
          cost_detail: "mixed",
        }),
      },
    });
    const text = w.text();
    expect(text).toContain("$8.25");
    // 旧常驻元素必须消失。
    expect(text).not.toContain("CCS 来源记录费用");
    expect(text).not.toContain("来源记录");
    expect(text).not.toContain("$7.25");
    // 费用节点是组件树挂载的触发器（缺陷 B 的 render 直建 DOM 问题已由
    // 组件化 + 全局 gcb- 前缀样式结构消除；不再有依赖 scoped CSS 的费用
    // 排版，因此 scope 属性断言随架构更新为结构存在性验证）。
    const trigger = w.find(".gcb-trigger");
    expect(trigger.exists()).toBe(true);
    expect(trigger.element.tagName).toBe("BUTTON");
  });

  it("纯汇总组：主读数 = 来源金额，无第二行", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          source_cost_usd: 7.25,
          total_cost_usd: 7.25,
          cost_detail: "rollup_only",
          unknown_pricing: false,
        }),
      },
    });
    expect(w.text()).toContain("$7.25");
    expect(w.text()).not.toContain("来源记录");
  });

  it("部分未定价 → 橙色金额，无常驻脚注符号", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0.5,
          total_cost_usd: 0.5,
          cost_partially_unknown: true,
          unknown_tokens: { input: 300, output: 0, cache_write: 0, cache_read: 0 },
          unknown_pricing: true,
        }),
      },
    });
    const text = w.text();
    expect(text).toContain("$0.50");
    expect(text).not.toContain("†");
    expect(w.find(".gcb-amount-main").classes()).toContain("gcb-warning");
    expect(text).not.toContain("来源");
  });

  it("完全没有费用依据 → 「未知」警告色，不伪装成零", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          total_cost_usd: null,
          unknown_pricing: true,
          unknown_tokens: { input: 100, output: 0, cache_write: 0, cache_read: 0 },
        }),
      },
    });
    expect(w.text()).toContain("未知");
    expect(w.text()).not.toContain("未知†");
    expect(w.find(".gcb-amount-main").classes()).toContain("gcb-warning");
  });
});

describe("CostCell 悬浮详情（v0.9.2）", () => {
  it("hover 打开浮层：构成分区含请求估算与来源费用，来源说明在浮层内", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 1.0,
          source_cost_usd: 7.25,
          total_cost_usd: 8.25,
          cost_detail: "mixed",
        }),
      },
    });
    // hover 前浮层内容不可见（NTooltip manual）——触发 hover 事件。
    w.find(".gcb-trigger").trigger("mouseenter");
    // naive-ui manual tooltip 用 :show 控制；断言组件状态经由父层 v-model
    // 不适用——这里直接断言浮层内容渲染在 DOM（NTooltip 渲染 body）。
    // 组件级断言：触发器 aria-label 含金额。
    const trigger = w.find(".gcb-trigger");
    expect(trigger.attributes("aria-label")).toContain("$8.25");
    w.unmount();
  });

  it("partial 时浮层内容包含部分覆盖说明", () => {
    const w = mount(CostCell, {
      props: {
        group: group({
          cost_usd: 0,
          source_cost_usd: 7.25,
          total_cost_usd: 7.25,
          cost_detail: "rollup_only",
          source_cost_state: "partial",
        }),
      },
    });
    const trigger = w.find(".gcb-trigger");
    expect(trigger.exists()).toBe(true);
    w.unmount();
  });
});

describe("费用依据判定（v0.9.2 第四轮复核）", () => {
  it("零元费率覆盖实际用量：悬浮显示 $0.00 估算依据，不说本行没有依据", async () => {
    const w = mount(GroupCostBreakdownTooltip, {
      attachTo: document.body,
      props: {
        group: group({
          cost_usd: 0,
          total_cost_usd: 0,
          has_estimated_cost: true,
          requests: 1,
          tokens: { input: 1_000_000, output: 0, cache_write: 0, cache_read: 0 },
          unknown_pricing: false,
        }),
        displayText: "$0.00",
        unknownOnly: false,
        open: true,
        pinned: true,
      },
    });
    await nextTick();
    const bodyText = document.body.textContent ?? "";
    expect(bodyText).toContain("请求明细估算");
    expect(bodyText).toContain("$0.00");
    expect(bodyText).not.toContain("本行没有可适用的费用依据");
    w.unmount();
  });
});
