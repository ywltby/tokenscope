// v0.9.3：聚合标题浮层——表后两组说明移到「聚合」标题的同一个浮层。
// 验证内容归属、鼠标移入、键盘固定/Escape、aria 关联与表格后无重复栏目。
import { beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import AggregateInfoTooltip from "./AggregateInfoTooltip.vue";
import UsageTable from "./UsageTable.vue";
import type { Group, SummaryReport } from "../types";

const sourceLines = [{ agent: "Claude Code", parts: ["已采集文件 12", "已保存请求 34"] }];
const selectionNotes = ["有 2 个日期/模型桶按请求明细统计，汇总不叠加。"];

function mountTip() {
  return mount(AggregateInfoTooltip, {
    attachTo: document.body,
    props: { sourceLines, selectionNotes },
    slots: { default: "聚合" },
  });
}

// PR #72 回归：鼠标离开与键盘失焦必须各自只清除自己的状态。
// 反例 1：标题仍持焦时移开鼠标，共享定时器把 focus 一并清掉，浮层误关。
// 反例 2：指针仍在浮层内时移走键盘焦点，共享定时器把 hover 一并清掉，浮层误关。
describe("聚合标题浮层：鼠标与键盘状态隔离（PR #72 回归）", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("标题仍持焦时移开鼠标，浮层保持打开", async () => {
    const w = mountTip();
    const trigger = w.find(".agg-info-trigger");
    await trigger.trigger("focus");
    await trigger.trigger("mouseenter");
    await nextTick();
    expect(trigger.attributes("aria-expanded")).toBe("true");
    await trigger.trigger("mouseleave");
    await new Promise((resolve) => setTimeout(resolve, 180));
    await nextTick();
    expect(trigger.attributes("aria-expanded")).toBe("true");
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    w.unmount();
  });

  it("指针仍在浮层内时移走键盘焦点，浮层保持打开", async () => {
    const w = mountTip();
    const trigger = w.find(".agg-info-trigger");
    await trigger.trigger("mouseenter");
    await nextTick();
    const bodyEl = document.querySelector(".agg-info-body") as HTMLElement | null;
    expect(bodyEl).not.toBeNull();
    // 鼠标从标题穿过间隙进入浮层
    await trigger.trigger("mouseleave");
    bodyEl?.dispatchEvent(new MouseEvent("mouseenter"));
    await trigger.trigger("focus");
    await trigger.trigger("blur");
    await new Promise((resolve) => setTimeout(resolve, 180));
    await nextTick();
    expect(trigger.attributes("aria-expanded")).toBe("true");
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    // 指针也离开后才关闭
    bodyEl?.dispatchEvent(new MouseEvent("mouseleave"));
    await new Promise((resolve) => setTimeout(resolve, 180));
    await nextTick();
    expect(trigger.attributes("aria-describedby")).toBeUndefined();
    w.unmount();
  });

  it("Escape 关闭、点击固定与固定态外点关闭不受隔离影响", async () => {
    const w = mountTip();
    const trigger = w.find(".agg-info-trigger");
    await trigger.trigger("click"); // 固定
    await trigger.trigger("blur");
    await nextTick();
    expect(trigger.attributes("aria-expanded")).toBe("true");
    document.body.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await nextTick();
    expect(trigger.attributes("aria-expanded")).toBe("false");
    await trigger.trigger("focus");
    await trigger.trigger("click"); // 再固定
    await nextTick();
    expect(trigger.attributes("aria-expanded")).toBe("true");
    await trigger.trigger("keydown", { key: "Escape" });
    await nextTick();
    expect(trigger.attributes("aria-expanded")).toBe("false");
    w.unmount();
  });
});

describe("聚合标题浮层（v0.9.3）", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("同一浮层含两个小标题与内容；触发器只有标题文字", async () => {
    const w = mountTip();
    await w.find(".agg-info-trigger").trigger("mouseenter");
    await nextTick();
    const body = document.body.textContent ?? "";
    expect(body).toContain("来源采集统计");
    expect(body).toContain("统计说明");
    expect(body).toContain("已采集文件 12");
    expect(body).toContain("有 2 个日期/模型桶按请求明细统计");
    expect(w.find(".agg-info-trigger").text()).toBe("聚合");
    w.unmount();
  });

  it("鼠标：悬浮打开，移入浮层保持，离开后关闭", async () => {
    const w = mountTip();
    const trigger = w.find(".agg-info-trigger");
    await trigger.trigger("mouseenter");
    await nextTick();
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    // 浮层被 Naive 传送到 body，不在组件 wrapper 内；直接取 DOM 派发事件。
    const bodyEl = document.querySelector(".agg-info-body") as HTMLElement | null;
    expect(bodyEl).not.toBeNull();
    await trigger.trigger("mouseleave");
    bodyEl?.dispatchEvent(new MouseEvent("mouseenter"));
    await new Promise((resolve) => setTimeout(resolve, 180));
    await nextTick();
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    bodyEl?.dispatchEvent(new MouseEvent("mouseleave"));
    await new Promise((resolve) => setTimeout(resolve, 180));
    await nextTick();
    expect(trigger.attributes("aria-describedby")).toBeUndefined();
    w.unmount();
  });

  it("键盘：focus 打开、Enter 固定、Escape 关闭且不移动焦点", async () => {
    const w = mountTip();
    const trigger = w.find(".agg-info-trigger");
    expect(trigger.element.tagName).toBe("BUTTON");
    await trigger.trigger("focus");
    await nextTick();
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    await trigger.trigger("click"); // 原生 button 的 Enter 等价
    await trigger.trigger("blur");
    await nextTick();
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    await trigger.trigger("keydown", { key: "Escape" });
    await nextTick();
    expect(trigger.attributes("aria-describedby")).toBeUndefined();
    expect(w.emitted("escape") === undefined).toBe(true); // 内部自处理，不对外泄漏
    w.unmount();
  });

  it("聚合卡后不再渲染两个独立栏目", () => {
    const g: Group = {
      key: "m1",
      requests: 1,
      tokens: { input: 10, output: 5, cache_write: 0, cache_read: 0 },
      cost_usd: 0.01,
      total_cost_usd: 0.01,
      unknown_pricing: false,
      unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    };
    const report: SummaryReport = {
      query_id: "q",
      pricing_revision: "rev",
      timezone: "UTC",
      generated_at: "t",
      sources: [],
      by: "model",
      groups: [g],
      totals: { ...g },
      warnings: [],
    };
    const w = mount(UsageTable, { props: { report } });
    const text = w.text();
    expect(text).not.toContain("来源采集统计");
    expect(text).not.toContain("统计说明");
    expect(text).toContain("点击行查看请求明细");
    w.unmount();
  });
});
