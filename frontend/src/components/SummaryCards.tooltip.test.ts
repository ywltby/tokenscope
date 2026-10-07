// F08（修复后复核 Task 7）：命中率公式 tooltip 的**真实浮层**验收——
// 不打桩 naive-ui，挂载真实 NTooltip，等待真实 Teleport/触发时序，
// 断言公式在初始隐藏、focus/hover 后出现在文档中、Escape 后关闭；
// 另测"从触发器移入内容不立即消失"与"focus 保持时鼠标离开不取消键盘展示"。
// 不能只看 aria-expanded，也不能靠透传 stub 检测可见性。
import { afterEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import SummaryCards from "./SummaryCards.vue";
import type { Group } from "../types";

function totals(): Group {
  return {
    key: "totals",
    requests: 12,
    tokens: { input: 12000, output: 3000, cache_write: 4000, cache_read: 50000 },
    cost_usd: 0.0126,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  };
}

function formulaVisible(): boolean {
  // naive-ui 隐藏浮层用 display:none（元素保留在 DOM）——textContent 检测
  // 不到 CSS 可见性，必须沿祖先链检查 display。
  const tips = Array.from(document.querySelectorAll<HTMLElement>(".hit-tip"));
  return tips.some((el) => {
    let node: HTMLElement | null = el;
    while (node) {
      if (getComputedStyle(node).display === "none") return false;
      node = node.parentElement;
    }
    return true;
  });
}

function formulaInDocument(): boolean {
  return formulaVisible();
}

async function waitTip(shown: boolean): Promise<void> {
  // 真实 NTooltip 的 Teleport 渲染需要等渲染时序；轮询至断言状态达成
  const deadline = Date.now() + 1000;
  while (Date.now() < deadline) {
    if (formulaInDocument() === shown) return;
    await flushPromises();
    await new Promise((r) => setTimeout(r, 10));
  }
  expect(formulaInDocument(), `公式应${shown ? "出现" : "消失"}于文档`).toBe(shown);
}

function mountReal() {
  return mount(SummaryCards, { props: { totals: totals() }, attachTo: document.body });
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("命中率公式真实浮层（F08）", () => {
  it("初始隐藏，focus 后出现，Escape 后关闭", async () => {
    const w = mountReal();
    const trigger = w.find(".metric-label-help");
    expect(trigger.exists()).toBe(true);
    await flushPromises();
    expect(formulaInDocument()).toBe(false); // 初始不渲染浮层内容
    await trigger.trigger("focus");
    await waitTip(true);
    expect(trigger.attributes("aria-expanded")).toBe("true");
    await trigger.trigger("keydown", { key: "Escape" });
    await waitTip(false);
    expect(trigger.attributes("aria-expanded")).toBe("false");
    w.unmount();
  });

  it("hover 打开、鼠标离开关闭", async () => {
    const w = mountReal();
    const trigger = w.find(".metric-label-help");
    await trigger.trigger("mouseenter");
    await waitTip(true);
    await trigger.trigger("mouseleave");
    await waitTip(false);
    w.unmount();
  });

  it("从触发器移入浮层内容不立即消失", async () => {
    const w = mountReal();
    const trigger = w.find(".metric-label-help");
    await trigger.trigger("mouseenter");
    await waitTip(true);
    // 指针从触发器移向浮层：先离开触发器，再进入内容（真实指针顺序）。
    // naive-ui 可能渲染多份内容副本（测量 + 显示），逐份派发。
    await trigger.trigger("mouseleave");
    const tips = document.querySelectorAll<HTMLElement>(".hit-tip");
    expect(tips.length).toBeGreaterThan(0);
    for (const tip of tips) {
      tip.dispatchEvent(new MouseEvent("mouseenter", { bubbles: false }));
    }
    await flushPromises();
    expect(formulaInDocument()).toBe(true);
    w.unmount();
  });

  it("focus 保持时鼠标离开不取消键盘展示", async () => {
    const w = mountReal();
    const trigger = w.find(".metric-label-help");
    await trigger.trigger("focus");
    await waitTip(true);
    // 键盘用户 focus 后指针恰好扫过触发器再离开：focus 未失，展示保持
    await trigger.trigger("mouseenter");
    await trigger.trigger("mouseleave");
    await flushPromises();
    expect(formulaInDocument()).toBe(true);
    expect(trigger.attributes("aria-expanded")).toBe("true");
    await trigger.trigger("blur");
    await waitTip(false);
    w.unmount();
  });
});
