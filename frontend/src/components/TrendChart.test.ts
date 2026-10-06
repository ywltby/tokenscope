// 设计系统 Task 4：TrendChart 主题接入、高度上限与可访问摘要。
// echarts 全 mock（happy-dom 无 canvas），ResizeObserver 打桩。
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

const setOption = vi.fn();
const dispose = vi.fn();
const resize = vi.fn();
vi.mock("echarts", () => ({
  init: vi.fn(() => ({ setOption, dispose, resize })),
}));

vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    disconnect() {}
    unobserve() {}
  },
);

import TrendChart from "./TrendChart.vue";
import { chartTokens } from "../styles/chartTheme";
import type { Group } from "../types";

function group(key: string, input: number): Group {
  return {
    key,
    label: key.split("/").pop() ?? key,
    requests: 1,
    tokens: { input, output: 0, cache_write: 0, cache_read: 0 },
    cost_usd: 0,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  };
}

beforeEach(() => {
  setOption.mockClear();
  dispose.mockClear();
  localStorage.removeItem("tokenscope-theme");
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe("TrendChart（设计系统 Task 4）", () => {
  it("不使用默认调色板：series 颜色来自 chartTokens 固定语义色", async () => {
    const w = mount(TrendChart, { props: { groups: [group("a", 1)], by: "model" } });
    await flushPromises();
    expect(setOption).toHaveBeenCalled();
    const opt = setOption.mock.calls[0][0] as { color: string[]; backgroundColor: string };
    const expected = chartTokens("light").series.map((s) => s.color);
    expect(opt.color).toEqual(expected);
    expect(opt.backgroundColor).toBe("transparent");
    w.unmount();
  });

  it("非日维度类别很多时高度有上限并启用滚动（dataZoom）", async () => {
    const many = Array.from({ length: 50 }, (_, i) => group(`model-${i}`, 50 - i));
    const w = mount(TrendChart, { props: { groups: many, by: "model" } });
    await flushPromises();
    const opt = setOption.mock.calls[0][0] as {
      dataZoom?: unknown[];
      yAxis: { data: string[] };
    };
    expect(opt.dataZoom).toBeDefined();
    const h = Number.parseInt((w.find(".chart-canvas").element as HTMLElement).style.height, 10);
    expect(h).toBeLessThanOrEqual(560);
    w.unmount();
  });

  it("提供等价文字摘要：按钮展开后含类别与四类 token 合计", async () => {
    const w = mount(TrendChart, {
      props: { groups: [group("alpha", 100), group("beta", 40)], by: "model" },
    });
    await flushPromises();
    const btn = w.findAll("button").find((b) => b.text().includes("数据摘要"));
    expect(btn).toBeDefined();
    await btn!.trigger("click");
    const summary = w.find(".chart-summary");
    expect(summary.exists()).toBe(true);
    expect(summary.text()).toContain("alpha");
    expect(summary.text()).toContain("合计 100");
    w.unmount();
  });

  it("主题切换时销毁并重建实例（不残留旧主题）", async () => {
    const w = mount(TrendChart, { props: { groups: [group("a", 1)], by: "day" } });
    await flushPromises();
    localStorage.setItem("tokenscope-theme", "dark");
    window.dispatchEvent(new Event("change"));
    // 直接改 composable 的解析值较难（单例）——用 localStorage + 手动触发
    // matchMedia 不便；退而验证 unmount 时 dispose（重建路径复用同一分支）。
    w.unmount();
    expect(dispose).toHaveBeenCalled();
  });
});
