// 汇总费用浮层（GroupCostBreakdownTooltip）：缓存缺价按零估算的披露。
//
// 背景（v0.9.2 第十一轮后端修复）：日汇总在「金额可确定、但任一可能胜出路径
// 按 0 估算」时保留金额与 unknown=0，同时置 Group.cache_price_zero_estimated
// =true、complete=false。浮层此前不读该字段，只显示通用估算说明，用户看不到
// 「该分项按 $0 估算」的依据。
//
// 断言口径：同一份 $2.00、未定价 token 全 0 的数据，只切换估算标记，分别验证
// 浮层有无说明；金额区域始终只有一个金额，说明只存在于浮层内。
import { beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import GroupCostBreakdownTooltip from "./GroupCostBreakdownTooltip.vue";
import type { Group } from "../types";

function group(opts: Partial<Group> = {}): Group {
  return {
    key: "mixed-zero",
    requests: 2,
    tokens: { input: 0, output: 0, cache_write: 1_000_000, cache_read: 1_000_000 },
    cost_usd: 2.0,
    total_cost_usd: 2.0,
    has_estimated_cost: true,
    cost_detail: "rollup_only",
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    ...opts,
  };
}

function mountOpen(overrides: Partial<Group> = {}) {
  return mount(GroupCostBreakdownTooltip, {
    attachTo: document.body,
    props: {
      group: group(overrides),
      displayText: "$2.00",
      unknownOnly: false,
      open: true,
      pinned: true,
    },
  });
}

describe("汇总费用浮层：缓存缺价按零估算", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("估算标记为 true → 浮层说明线上来源未提供缓存单价、该分项按 $0 估算", async () => {
    const w = mountOpen({ cache_price_zero_estimated: true });
    await nextTick();
    const body = document.body.textContent ?? "";
    expect(body).toContain("缺失缓存价按 0 估算");
    expect(body).toContain("未提供该缓存分项单价");
    expect(body).toContain("按 $0 参与估算");
    // 金额与完整性读数不受影响：没有未定价 token，不出现缺失区。
    expect(body).not.toContain("未计价 token");
    w.unmount();
  });

  it("同一份数据把估算标记改为 false（显式零价）→ 不显示该说明", async () => {
    const w = mountOpen({ cache_price_zero_estimated: false });
    await nextTick();
    const body = document.body.textContent ?? "";
    expect(body).not.toContain("缺失缓存价按 0 估算");
    expect(body).not.toContain("按 $0 参与估算");
    // 通用估算说明仍在（只采用可证明适用的费率）。
    expect(body).toContain("请求明细估算");
    w.unmount();
  });

  it("金额区域只有一个金额，说明不进常驻文案", async () => {
    const w = mountOpen({ cache_price_zero_estimated: true });
    await nextTick();
    const trigger = w.find(".gcb-trigger");
    expect(trigger.exists()).toBe(true);
    expect(trigger.text()).toBe("$2.00");
    expect(trigger.text()).not.toContain("估算");
    w.unmount();
  });

  it("交互不变：点击切换、Escape 关闭、聚焦上报", async () => {
    const w = mountOpen({ cache_price_zero_estimated: true });
    await nextTick();
    const trigger = w.find(".gcb-trigger");
    await trigger.trigger("click");
    await trigger.trigger("focus");
    await trigger.trigger("keydown", { key: "Escape" });
    expect(w.emitted("toggle")).toHaveLength(1);
    expect(w.emitted("focus-enter")).toHaveLength(1);
    expect(w.emitted("escape")).toHaveLength(1);
    w.unmount();
  });
});
