// 设计系统 Task 8：无障碍关键路径的自动化断言。
// 真机视觉/键盘走查记录见 docs/plans/active/2026-10-06-design-system-visual-qa.md。
import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

// App 挂载即监听 close-requested：mock 掉事件模块（真实 listen 依赖
// Tauri 运行时，happy-dom 无 __TAURI_INTERNALS__）。
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const passthrough = (name: string) =>
    dc({
      name,
      setup(_, { slots }) {
        return () => h("div", { class: `stub-${name}` }, slots.default?.());
      },
    });
  return {
    ...actual,
    NLayout: passthrough("NLayout"),
    NLayoutHeader: passthrough("NLayoutHeader"),
    NLayoutContent: passthrough("NLayoutContent"),
    NMessageProvider: passthrough("NMessageProvider"),
    NConfigProvider: passthrough("NConfigProvider"),
  };
});

// happy-dom 无 canvas：echarts 全 mock（与 TrendChart.test.ts 同口径）
const setOption = vi.fn();
const dispose = vi.fn();
const resize = vi.fn();
vi.mock("echarts", () => ({
  init: vi.fn(() => ({ setOption, dispose, resize })),
}));

vi.mock("./views/Dashboard.vue", () => ({
  default: defineComponent({
    name: "Dashboard",
    props: { refreshKey: { type: Number, default: 0 } },
    setup: () => () => h("div", { class: "stub-dashboard" }),
  }),
}));
vi.mock("./views/Settings.vue", () => ({
  default: defineComponent({
    name: "Settings",
    props: { refreshKey: { type: Number, default: 0 } },
    setup: () => () => h("div", { class: "stub-settings" }),
  }),
}));

import MainApp from "./MainApp.vue";
import SummaryCards from "./components/SummaryCards.vue";
import TrendChart from "./components/TrendChart.vue";
import type { Group } from "./types";

beforeAll(() => {
  localStorage.removeItem("tokenscope-theme");
  invokeMock.mockImplementation(() => Promise.resolve(null));
});

import { beforeAll } from "vitest";

describe("无障碍关键点（设计系统 Task 8）", () => {
  it("MainApp：分段控件 radiogroup 语义 + 主题控件带可访问名称", async () => {
    const w = mount(MainApp);
    await flushPromises();
    // 页面切换：第一个 radiogroup
    expect(w.find('[role="radiogroup"][aria-label="页面切换"]').exists()).toBe(true);
    // 主题切换：第二个 radiogroup
    expect(w.find('[role="radiogroup"][aria-label^="主题偏好"]').exists()).toBe(true);
    const radios = w.findAll('[role="radio"]');
    expect(radios.length).toBeGreaterThanOrEqual(5); // 至少 2 (页面) + 3 (主题)
    w.unmount();
  });

  it("SummaryCards：指标条与分项行都有可访问名称", () => {
    const totals: Group = {
      key: "totals",
      requests: 3,
      tokens: { input: 10, output: 5, cache_write: 0, cache_read: 2 },
      cost_usd: 0.5,
      unknown_pricing: false,
      unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    };
    const w = mount(SummaryCards, { props: { totals } });
    expect(w.find('[aria-label="用量指标"]').exists()).toBe(true);
    expect(w.find('[aria-label="token 分项"]').exists()).toBe(true);
    w.unmount();
  });

  it("TrendChart：图表 role=img 保留描述性 aria-label", async () => {
    const groups: Group[] = [
      {
        key: "a",
        label: "a",
        requests: 1,
        tokens: { input: 100, output: 0, cache_write: 0, cache_read: 0 },
        cost_usd: 0,
        unknown_pricing: false,
        unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
      },
    ];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
        unobserve() {}
      },
    );
    setOption.mockClear();
    const w = mount(TrendChart, { props: { groups, by: "day" } });
    await flushPromises();
    const canvas = w.find(".chart-canvas");
    expect(canvas.attributes("role")).toBe("img");
    expect(canvas.attributes("aria-label")).toBe("使用趋势");
    w.unmount();
    vi.unstubAllGlobals();
  });
});
