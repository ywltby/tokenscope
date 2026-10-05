// Task 3：全局首次同步横幅——needs_sync 时可见、可同步、失败保留；
// 有本地 models.dev 快照（离线可用）时不出现。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

// NAlert 打桩为透传（内容同渲染），被测对象是横幅的编排逻辑。
vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h: hh } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const NAlertStubComponent = dc({
    name: "NAlert",
    setup(_, { slots }) {
      return () => hh("div", { class: "alert-stub" }, slots.default?.());
    },
  });
  return { ...actual, NAlert: NAlertStubComponent };
});

import PricingStatusBanner from "./PricingStatusBanner.vue";

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
const statusOk = {
  ...statusNeedsSync,
  modelsdevAvailable: true,
  modelsdevCount: 7957,
  modelsdevSyncedAt: "2026-10-06T00:00:00Z",
  hasAnyPricing: true,
  needsSync: false,
};

beforeEach(() => {
  invokeMock.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
});

function statusCalls(): number {
  return invokeMock.mock.calls.filter((c) => c[0] === "pricing_status").length;
}

describe("PricingStatusBanner（Task 3）", () => {
  it("pricing_banner_visible_without_modelsdev_cache", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusNeedsSync);
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner);
    await flushPromises();
    expect(w.text()).toContain("尚未获取定价");
    expect(w.text()).toContain("立即同步");
  });

  it("pricing_banner_hidden_with_cached_modelsdev", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusOk);
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner);
    await flushPromises();
    expect(w.text()).not.toContain("尚未获取定价");
  });

  it("pricing_banner_sync_success_refreshes_status", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusNeedsSync);
      if (cmd === "sync_pricing_openrouter") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner);
    await flushPromises();
    const before = statusCalls();
    const sync = w.findAll("button").find((b) => b.text().includes("立即同步"));
    await sync!.trigger("click");
    await flushPromises();
    expect(statusCalls()).toBeGreaterThan(before);
    // 让第二次 pricing_status 返回已同步状态，验证横幅消失。
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusOk);
      return Promise.resolve(null);
    });
    await sync!.trigger("click");
    await flushPromises();
    expect(w.text()).not.toContain("尚未获取定价");
  });

  it("pricing_banner_sync_failure_keeps_warning", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusNeedsSync);
      if (cmd === "sync_pricing_openrouter") return Promise.reject(new Error("network down"));
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner);
    await flushPromises();
    const sync = w.findAll("button").find((b) => b.text().includes("立即同步"));
    await sync!.trigger("click");
    await flushPromises();
    expect(w.text()).toContain("同步失败");
    expect(w.text()).toContain("network down");
    expect(w.text()).toContain("尚未获取定价");
  });
});
