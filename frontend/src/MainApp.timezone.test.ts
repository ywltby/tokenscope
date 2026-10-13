// U03：主应用启动时序——先读取设置得到时区，时区就绪后才挂载汇总页；
// 时区就绪前不得发出任何查询（首屏 query_begin 的 tz 必须已就绪）。
// 这里挂载**真实** Dashboard（子组件打桩），因此 query_begin 计数是真实的；
// Settings 打桩只为验证它同样受时区门控。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

// 布局组件打桩；NButton/NSpin 等叶子组件保持真实（与 Dashboard.test.ts 一致）。
vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h } = await import("vue");
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
    NConfigProvider: passthrough("NConfigProvider"),
    NMessageProvider: passthrough("NMessageProvider"),
  };
});

vi.mock("./views/Settings.vue", () => ({
  default: defineComponent({
    name: "Settings",
    props: { refreshKey: { type: Number, default: 0 } },
    setup: () => () => h("div", { class: "stub-settings" }),
  }),
}));

import MainApp from "./MainApp.vue";
import { resetTimezoneForTests } from "./composables/timezone";

const queryHandle = {
  queryId: "q-tz-gate",
  generation: 1,
  pricingRevision: "rev-test",
  timezone: "UTC",
  asOf: "2026-10-13T00:00:00+08:00",
  range: null,
};

const summary = {
  query_id: "q-tz-gate",
  pricing_revision: "rev-test",
  timezone: "UTC",
  generated_at: "2026-10-13T00:00:00+08:00",
  sources: [],
  by: "day",
  groups: [],
  totals: {
    key: "合计",
    requests: 0,
    tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    cost_usd: 0,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  },
  warnings: [],
};

const events = {
  query_id: "q-tz-gate",
  pricing_revision: "rev-test",
  rows: [],
  total: 0,
  warnings: [],
};

function beginCalls(): [string, Record<string, unknown>][] {
  return invokeMock.mock.calls.filter((c) => c[0] === "query_begin") as [
    string,
    Record<string, unknown>,
  ][];
}

function mountApp() {
  return mount(MainApp, {
    global: {
      stubs: {
        SummaryCards: true,
        UsageTable: true,
        TrendChart: true,
        EventTable: true,
        AgentIcon: true,
        DateRangeSelect: true,
      },
    },
  });
}

beforeEach(() => {
  invokeMock.mockReset();
  localStorage.clear();
  resetTimezoneForTests();
});

describe("MainApp 时区就绪时序（U03）", () => {
  it("timezone_gate_blocks_queries：时区就绪前不挂载汇总页、不发 query_begin；就绪后只发一批", async () => {
    // 所有 settings_get（时区启动读取 + 设置页预读）共享同一闸门：release 前
    // 时区不确定，任何查询都不允许发出。
    let releaseSettings!: (v: unknown) => void;
    const settingsGate = new Promise((r) => (releaseSettings = r));
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") return settingsGate;
      if (cmd === "query_begin") return Promise.resolve(queryHandle);
      if (cmd === "query_summary") return Promise.resolve(summary);
      if (cmd === "query_events") return Promise.resolve(events);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mountApp();
    await flushPromises();
    expect(beginCalls(), "时区就绪前不得发任何查询").toHaveLength(0);
    expect(w.text(), "汇总页在时区就绪前不挂载").not.toContain("用量汇总");

    releaseSettings({ timezone: "UTC" });
    await flushPromises();
    expect(beginCalls(), "就绪后只发一批查询").toHaveLength(1);
    expect(beginCalls()[0][1], "首屏查询必须带上权威时区").toMatchObject({ tz: "UTC" });
    expect(w.text()).toContain("用量汇总");
    w.unmount();
  });

  it("settings_page_waits_for_timezone：设置页同样等时区就绪（下拉显示权威值，不先渲染默认再跳变）", async () => {
    let releaseSettings!: (v: unknown) => void;
    const settingsGate = new Promise((r) => (releaseSettings = r));
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") return settingsGate;
      return Promise.resolve(null);
    });
    const w = mountApp();
    await flushPromises();
    await w.get('[aria-label="页面切换"] [aria-label="设置"]').trigger("click");
    await flushPromises();
    expect(w.find(".stub-settings").exists(), "未就绪时不挂载设置页").toBe(false);

    releaseSettings({ timezone: "UTC" });
    await flushPromises();
    expect(w.find(".stub-settings").exists()).toBe(true);
    w.unmount();
  });
});
