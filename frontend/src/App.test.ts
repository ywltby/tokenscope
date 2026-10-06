// Task 4：App 集成测试——真实组件接线 + 结构化 needsSync 可见性
//（Tauri invoke 全 mock，重子组件打桩，不复制 Naive UI 内部实现）。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

// 布局类组件打桩：只验证 App 的接线（横幅 + 页面切换），不渲染 Naive UI 内部。
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

import App from "./App.vue";
import PricingStatusBanner from "./components/PricingStatusBanner.vue";

const statusNeedsSync = {
  modelsdevAvailable: false,
  modelsdevCount: 0,
  modelsdevSyncedAt: null,
  openrouterAvailable: false,
  externalCount: 0,
  hasAnyPricing: false,
  needsSync: true,
  warnings: [],
};
const statusOk = { ...statusNeedsSync, needsSync: false, hasAnyPricing: true };

function mockApp(pricingStatus: object): void {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "pricing_status") return Promise.resolve(pricingStatus);
    if (cmd === "view_cache_load") return Promise.resolve(null);
    if (cmd === "summarize") return Promise.resolve({ groups: [], totals: {}, sources: [] });
    if (cmd === "list_events") return Promise.resolve({ rows: [], total: 0, warnings: [] });
    if (cmd === "source_status") return Promise.resolve([]);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("App 集成（Task 4）", () => {
  it("needsSync=true 时全局横幅可见", async () => {
    mockApp(statusNeedsSync);
    const w = mount(App);
    await flushPromises();
    expect(w.text()).toContain("尚未获取定价");
    expect(w.findComponent(PricingStatusBanner).exists()).toBe(true);
  });

  it("needsSync=false 时不显示横幅", async () => {
    mockApp(statusOk);
    const w = mount(App);
    await flushPromises();
    expect(w.text()).not.toContain("尚未获取定价");
    expect(w.findComponent(PricingStatusBanner).exists()).toBe(true);
  });
});

describe("App 应用壳（设计系统 Task 2，苹果风格分段控件）", () => {
  beforeEach(() => {
    localStorage.removeItem("tokenscope-theme");
  });

  it("汇总/设置分段控件可识别且选中态可见", async () => {
    mockApp(statusOk);
    const w = mount(App);
    await flushPromises();
    const items = w.findAll('[role="radio"]');
    // 第一组是页面切换（汇总/设置），第二组是主题切换（☀/☾/自动）
    const pageItems = items.slice(0, 2);
    expect(pageItems.map((t) => t.text())).toEqual(["汇总", "设置"]);
    expect(pageItems[0].attributes("aria-checked")).toBe("true");
    await pageItems[1].trigger("click");
    await flushPromises();
    expect(pageItems[1].attributes("aria-checked")).toBe("true");
    expect(w.find(".stub-settings").exists()).toBe(true);
  });

  it("主题分段控件可读出当前偏好并驱动 data-theme", async () => {
    mockApp(statusOk);
    const w = mount(App);
    await flushPromises();
    const groups = w.findAll('[role="radiogroup"]');
    expect(groups.length).toBeGreaterThanOrEqual(2);
    const themeGroup = groups[1]; // 第二组是主题
    const themeItems = themeGroup.findAll('[role="radio"]');
    expect(themeItems.map((t) => t.text())).toEqual(["☀", "☾", "自动"]);
    // 默认 system（自动），解析为 light
    expect(themeItems[2].attributes("aria-checked")).toBe("true");
    expect(document.documentElement.dataset.theme).toBe("light");
    await themeItems[1].trigger("click");
    await flushPromises();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("tokenscope-theme")).toBe("dark");
  });

  it("横幅渲染在内容之前（不遮挡主体）", async () => {
    mockApp(statusNeedsSync);
    const w = mount(App);
    await flushPromises();
    const banner = w.find(".banner-slot").element;
    const content = w.find(".app-content").element;
    expect(
      banner.compareDocumentPosition(content) & Node.DOCUMENT_POSITION_FOLLOWING,
      "横幅必须在内容之前",
    ).toBeTruthy();
  });
});
