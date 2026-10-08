// Task 3：全局首次同步横幅——needs_sync 时可见、可同步、失败保留；
// 有本地 models.dev 快照（离线可用）时不出现。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";

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
// RC04：横幅在 window 上挂 pricing-status-changed 监听——未卸载的实例会
// 跨用例继续响应事件（曾让后续用例统计到 9 次 pricing_status 调用）。
// 自动卸载每个用例创建的 wrapper，保证监听与状态互不串扰。
enableAutoUnmount(afterEach);

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
    expect(w.text()).toContain("network down");
    expect(w.text()).toContain("尚未获取定价");
  });

  it("部分成功（主源 OK/补充源失败）→ 横幅消失但显示补充源失败原因", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusNeedsSync);
      if (cmd === "sync_pricing_openrouter")
        return Promise.reject(new Error("OpenRouter: network down"));
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner);
    await flushPromises();
    expect(w.text()).toContain("尚未获取定价");
    // 同步后状态刷新为主源可用（pricing_status 换为 OK，sync 仍失败）
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusOk);
      if (cmd === "sync_pricing_openrouter")
        return Promise.reject(new Error("OpenRouter: network down"));
      return Promise.resolve(null);
    });
    const sync = w.findAll("button").find((b) => b.text().includes("立即同步"));
    await sync!.trigger("click");
    await flushPromises();
    expect(w.text()).not.toContain("尚未获取定价");
    expect(w.text()).toContain("OpenRouter: network down");
  });

  it("pricing_status 读取失败 → 可重试提示而非静默空 DOM", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.reject(new Error("ipc broken"));
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner);
    await flushPromises();
    expect(w.text()).toContain("定价状态读取失败");
    expect(w.text()).toContain("ipc broken");
    // 重试成功后恢复
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusOk);
      return Promise.resolve(null);
    });
    const retry = w.findAll("button").find((b) => b.text().includes("重试"));
    await retry!.trigger("click");
    await flushPromises();
    expect(w.text()).not.toContain("定价状态读取失败");
  });

  it("监听 pricing-status-changed：设置页同步后横幅重新读取状态", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusNeedsSync);
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner);
    await flushPromises();
    expect(w.text()).toContain("尚未获取定价");
    // 设置页（另一组件）派发事件 + 状态已被其刷新为可用
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusOk);
      return Promise.resolve(null);
    });
    window.dispatchEvent(new Event("pricing-status-changed"));
    await flushPromises();
    expect(w.text()).not.toContain("尚未获取定价");
  });
});

describe("PricingStatusBanner 视觉（设计系统 Task 3）", () => {
  it("横幅使用内联通知条结构、状态图标与 alert 角色", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") return Promise.resolve(statusNeedsSync);
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner);
    await flushPromises();
    const notice = w.find(".ts-notice");
    expect(notice.exists()).toBe(true);
    expect(notice.find(".ts-notice-icon").exists()).toBe(true);
    expect(notice.findAll("button").some((b) => b.text().includes("立即同步"))).toBe(true);
    expect(w.find('[role="alert"]').exists()).toBe(true);
  });
});

// ── UX06：横幅错误恢复——同步重试与状态读取重试各有 pending/防重复；
// 两类错误同时可见可操作；状态重试不触发联网；部分同步保留可用价格。──
describe("PricingStatusBanner 错误恢复（UX06）", () => {
  function mockStatus(needsSync = true) {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status")
        return Promise.resolve({
          modelsdevAvailable: !needsSync,
          modelsdevCount: needsSync ? 0 : 5,
          modelsdevSyncedAt: needsSync ? null : "t",
          openrouterAvailable: false,
          externalCount: 0,
          hasAnyPricing: !needsSync,
          needsSync,
          warnings: [],
        });
      if (cmd === "sync_pricing_openrouter")
        return Promise.resolve([{ source: "models.dev", count: 5 }]);
      return Promise.resolve(null);
    });
  }

  it("status_retry_never_starts_sync：状态重试只调 pricing_status，绝不触发同步", async () => {
    let statusFail = true;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") {
        if (statusFail) {
          statusFail = false;
          return Promise.reject(new Error("status down"));
        }
        return Promise.resolve({
          modelsdevAvailable: true,
          modelsdevCount: 5,
          modelsdevSyncedAt: "t",
          openrouterAvailable: false,
          externalCount: 0,
          hasAnyPricing: true,
          needsSync: false,
          warnings: [],
        });
      }
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    expect(w.text()).toContain("定价状态读取失败");
    invokeMock.mockClear();
    const retry = w.findAll("button").find((b) => b.text() === "重试");
    await retry!.trigger("click");
    await flushPromises();
    // 只发起 pricing_status，绝不触发联网同步。
    const cmds = invokeMock.mock.calls.map((c) => c[0]);
    expect(cmds).toEqual(["pricing_status"]);
    expect(w.text()).not.toContain("定价状态读取失败");
    w.unmount();
    document.body.innerHTML = "";
  });

  it("retry_ignores_duplicate_activation：进行中再点不发起第二次请求", async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    let calls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") {
        calls += 1;
        return gate.then(() =>
          Promise.resolve({
            modelsdevAvailable: true,
            modelsdevCount: 5,
            modelsdevSyncedAt: "t",
            openrouterAvailable: false,
            externalCount: 0,
            hasAnyPricing: true,
            needsSync: false,
            warnings: [],
          }),
        );
      }
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    expect(calls).toBe(1); // 挂载首次读取（在途，gate 未释放）
    // 在途时重复激活（等价连点重试/并发刷新）→ 不发起第二次。
    const vm = w.vm as unknown as { refreshStatus: () => Promise<void> };
    await vm.refreshStatus();
    await vm.refreshStatus();
    expect(calls, "在途时重复激活不得发起第二次请求").toBe(1);
    release();
    await flushPromises();
    expect(calls).toBe(1);
    w.unmount();
    document.body.innerHTML = "";
  });

  it("partial_sync_preserves_usable_pricing：同步失败但主源可用 → 错误可见且费用正常", async () => {
    mockStatus(true);
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    // 同步失败（网络）但状态刷新显示主源可用 → 横幅收敛、错误保留。
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "sync_pricing_openrouter") return Promise.reject(new Error("network down"));
      if (cmd === "pricing_status")
        return Promise.resolve({
          modelsdevAvailable: true,
          modelsdevCount: 5,
          modelsdevSyncedAt: "t",
          openrouterAvailable: false,
          externalCount: 0,
          hasAnyPricing: true,
          needsSync: false,
          warnings: [],
        });
      return Promise.resolve(null);
    });
    const sync = w.findAll("button").find((b) => b.text().includes("立即同步"));
    await sync!.trigger("click");
    await flushPromises();
    expect(w.text()).toContain("同步部分失败");
    expect(w.text()).toContain("network down");
    expect(w.text()).not.toContain("尚未获取定价");
    w.unmount();
    document.body.innerHTML = "";
  });

  it("sync_failure_without_status_does_not_claim_partial_success：状态未知不宣称部分成功，两类错误同时可见", async () => {
    let syncAttempted = false;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "sync_pricing_openrouter") {
        syncAttempted = true;
        return Promise.reject(new Error("sync down"));
      }
      if (cmd === "pricing_status") {
        if (syncAttempted) return Promise.reject(new Error("status down"));
        return Promise.resolve({
          modelsdevAvailable: false,
          modelsdevCount: 0,
          modelsdevSyncedAt: null,
          openrouterAvailable: false,
          externalCount: 0,
          hasAnyPricing: false,
          needsSync: true,
          warnings: [],
        });
      }
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    const sync = w.findAll("button").find((b) => b.text().includes("立即同步"));
    await sync!.trigger("click");
    await flushPromises();
    // RC04：状态未知时不得宣称"部分成功"——只能报"同步失败"。
    expect(w.text()).toContain("同步失败");
    expect(w.text()).not.toContain("同步部分失败");
    expect(w.text()).toContain("sync down");
    expect(w.text()).toContain("定价状态读取失败");
    expect(w.text()).toContain("status down");
    w.unmount();
    document.body.innerHTML = "";
  });

  // ── RC04：同步错误必须有可操作的重试动作；状态刷新意图不丢失 ──
  it("partial_sync_failure_keeps_sync_retry_action：主源可用时仍可重试补充源同步", async () => {
    // 首载：主源缺失（needsSync）→ 点"立即同步"→ 同步失败但状态显示主源
    // 已可用（needsSync=false）。横幅收敛，但失败条必须带**重试同步**按钮。
    let syncCalls = 0;
    let synced = false;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "sync_pricing_openrouter") {
        syncCalls += 1;
        if (syncCalls === 1) {
          synced = true;
          return Promise.reject(new Error("OpenRouter: network down"));
        }
        return Promise.resolve([{ source: "models.dev", count: 5 }]);
      }
      if (cmd === "pricing_status")
        return Promise.resolve(
          synced
            ? {
                modelsdevAvailable: true,
                modelsdevCount: 5,
                modelsdevSyncedAt: "t",
                openrouterAvailable: false,
                externalCount: 0,
                hasAnyPricing: true,
                needsSync: false,
                warnings: [],
              }
            : {
                modelsdevAvailable: false,
                modelsdevCount: 0,
                modelsdevSyncedAt: null,
                openrouterAvailable: false,
                externalCount: 0,
                hasAnyPricing: false,
                needsSync: true,
                warnings: [],
              },
        );
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text().includes("立即同步"))!
      .trigger("click");
    await flushPromises();
    expect(w.text()).toContain("同步部分失败");
    expect(w.text()).toContain("OpenRouter: network down");
    expect(w.text()).not.toContain("尚未获取定价");

    // 真实点击错误条的"重试同步" → 再次调用同步；成功后才清理该错误。
    const retry = w.findAll("button").find((b) => b.text().includes("重试同步"));
    expect(retry, "同步失败条必须有可操作的重试同步按钮").toBeDefined();
    await retry!.trigger("click");
    await flushPromises();
    expect(syncCalls, "重试必须再次调用同步").toBe(2);
    expect(w.text()).not.toContain("OpenRouter: network down");
    expect(w.text()).not.toContain("同步部分失败");
    w.unmount();
    document.body.innerHTML = "";
  });

  it("repeated_sync_activation_is_single_flight：同步进行中重复激活只发一次请求", async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    let syncCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "sync_pricing_openrouter") {
        syncCalls += 1;
        return gate.then(() => Promise.resolve([]));
      }
      if (cmd === "pricing_status") return Promise.resolve(statusNeedsSync);
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    const sync = w.findAll("button").find((b) => b.text().includes("立即同步"))!;
    await sync.trigger("click");
    expect(syncCalls).toBe(1);
    // 在途时重复点击（等价快速连点/程序化触发）→ 不得并发第二次
    await sync.trigger("click");
    await sync.trigger("click");
    expect(syncCalls).toBe(1);
    release();
    await flushPromises();
    expect(syncCalls).toBe(1);
    w.unmount();
    document.body.innerHTML = "";
  });

  it("status_change_during_pending_read_is_not_lost：挂起期间的同步完成不被丢弃", async () => {
    // 状态读取挂起时同步完成 → 释放旧读取后最终必须采用**同步后**的状态。
    const [pendingRead, resolvePendingRead] = ((): [Promise<unknown>, (v: unknown) => void] => {
      let r!: (v: unknown) => void;
      const p = new Promise<unknown>((res) => (r = res));
      return [p, r];
    })();
    let statusCalls = 0;
    let synced = false;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") {
        statusCalls += 1;
        if (statusCalls === 1) return pendingRead; // 首次读取挂起
        return Promise.resolve(
          synced
            ? {
                modelsdevAvailable: true,
                modelsdevCount: 9,
                modelsdevSyncedAt: "t",
                openrouterAvailable: true,
                externalCount: 0,
                hasAnyPricing: true,
                needsSync: false,
                warnings: [],
              }
            : statusNeedsSync,
        );
      }
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    expect(statusCalls).toBe(1);
    // 挂起期间外部派发"状态已变化"（等价设置页同步完成：状态已更新）
    synced = true;
    window.dispatchEvent(new Event("pricing-status-changed"));
    await flushPromises();
    expect(statusCalls, "在途时不得立即并发第二次").toBe(1);
    // 释放旧读取（返回同步前的旧状态）
    resolvePendingRead(statusNeedsSync);
    await flushPromises();
    // 补读一次 → 最终采用同步后的状态（横幅消失）
    expect(statusCalls, "结束后必须补读一次").toBe(2);
    expect(w.text()).not.toContain("尚未获取定价");
    w.unmount();
    document.body.innerHTML = "";
  });

  it("sync_and_status_pending_states_are_separate：两类按钮 pending 互不阻塞", async () => {
    // RC04：同步失败与状态失败各有独立 pending——状态"重试"在途时，错误条
    // 的"重试同步"既不显示 loading，也仍可发起同步（不能把一种 pending
    // 当成另一种，也不能让一种错误挡住另一种的重试入口）。
    let releaseStatus!: (v: unknown) => void;
    const statusGate = new Promise<unknown>((r) => (releaseStatus = r));
    let statusCalls = 0;
    let syncCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") {
        statusCalls += 1;
        if (statusCalls === 1) return Promise.resolve({ ...statusNeedsSync }); // 首载：需要同步
        if (statusCalls === 2) return Promise.reject(new Error("status down")); // 同步后刷新失败
        return statusGate; // 状态重试挂起中
      }
      if (cmd === "sync_pricing_openrouter") {
        syncCalls += 1;
        return Promise.reject(new Error("sync down"));
      }
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    // 点"立即同步"→ 同步失败 + 状态刷新失败 → 两条错误同时可见。
    await w
      .findAll("button")
      .find((b) => b.text().includes("立即同步"))!
      .trigger("click");
    await flushPromises();
    expect(w.text()).toContain("sync down");
    expect(w.text()).toContain("定价状态读取失败");

    const statusRetry = w.findAll("button").find((b) => b.text() === "重试")!;
    const syncRetry = w.findAll("button").find((b) => b.text().includes("重试同步"))!;
    // 状态重试进入在途
    await statusRetry.trigger("click");
    await flushPromises();
    expect(statusCalls, "状态重试应发起一次读取").toBe(3);
    // 该读取挂起期间：同步按钮不受影响——无 loading，且点击仍能发起同步
    expect(syncRetry.classes(), "状态 pending 不得点亮同步按钮").not.toContain("n-button--loading");
    const syncBefore = syncCalls;
    await syncRetry.trigger("click");
    expect(syncCalls, "状态 pending 不得阻塞同步重试").toBe(syncBefore + 1);
    releaseStatus({ ...statusOk });
    await flushPromises();
    expect(w.text(), "状态读取成功后错误条收敛").not.toContain("定价状态读取失败");
    expect(w.text()).toContain("sync down");
    expect(syncRetry.classes()).not.toContain("n-button--loading");
    w.unmount();
    document.body.innerHTML = "";
  });

  it("unmount_invalidates_inflight_requests_and_listeners：卸载后晚到响应与事件都不写状态", async () => {
    // RC04：卸载使在途请求失效并移除监听。
    let release!: (v: unknown) => void;
    const gate = new Promise<unknown>((r) => (release = r));
    let calls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_status") {
        calls += 1;
        return calls === 1 ? gate : Promise.resolve(statusOk);
      }
      return Promise.resolve(null);
    });
    const w = mount(PricingStatusBanner, { attachTo: document.body });
    await flushPromises();
    expect(calls).toBe(1);
    w.unmount();
    // 卸载后释放挂起的读取：不得抛未处理 rejection，也不得再响应外部事件。
    release(statusNeedsSync);
    await flushPromises();
    window.dispatchEvent(new Event("pricing-status-changed"));
    await flushPromises();
    expect(calls, "卸载后监听已移除，事件不再触发读取").toBe(1);
    document.body.innerHTML = "";
  });
});
