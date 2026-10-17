// Task 4（审阅计划）：Settings 组件级行为测试——
// 同步失败仍刷新价格视图、派发 pricing-status-changed、来源重叠错误
// 可见且输入保留、重新加载不清除未提交草稿。
// IPC 全 mock；Naive UI 布局组件打桩，只验证行为与事件。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, shallowMount, type VueWrapper } from "@vue/test-utils";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

// 布局/表单组件打桩
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
    NCard: passthrough("NCard"),
    NGrid: passthrough("NGrid"),
    NGi: passthrough("NGi"),
    NSpin: passthrough("NSpin"),
    NAlert: passthrough("NAlert"),
    NCollapse: passthrough("NCollapse"),
    NCollapseItem: passthrough("NCollapseItem"),
    NDataTable: passthrough("NDataTable"),
    NStatistic: passthrough("NStatistic"),
  };
});

// useMessage 打桩（naive-ui 的 useMessage 需要 provider）；
// spy 提升为稳定引用，供断言成功/失败提示。
const msgSpy = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), info: vi.fn() }));
vi.mock("naive-ui", async (importOriginal) => {
  const actual = await importOriginal<typeof import("naive-ui")>();
  return {
    ...actual,
    useMessage: () => msgSpy,
  };
});

import Settings from "./Settings.vue";
import { createSettingsPreload, SETTINGS_PRELOAD } from "../lib/settingsPreload";
import {
  COLLECT_INTERVAL_OPTIONS,
  resetCollectIntervalForTests,
  useCollectInterval,
} from "../composables/collectInterval";
// UX08：样式契约断言需要 SFC 源文本（项目无 @types/node，用 Vite ?raw）
import settingsSource from "./Settings.vue?raw";

const pricingView = {
  path: "C:/pricing.toml",
  modelsdev_path: "C:/md.json",
  modelsdev_synced_at: "2026-10-06T00:00:00Z",
  modelsdev_count: 10,
  openrouter_path: "C:/or.json",
  openrouter_synced_at: null,
  openrouter_count: 0,
  external_count: 0,
  entries: [],
  warnings: [],
};

/** PR3：source_paths_list 的行（后端已解析身份/标签/状态；前端只展示）。 */
const sourceRowsFixture = [
  {
    id: "c:/claude-logs",
    agent: "claude",
    path: "C:/claude-logs",
    canonical_identity: "c:/claude-logs",
    identity_confirmed: true,
    origins: ["默认自动"],
    mapping_path: null,
    row_enabled: true,
    enabled: true,
    state: "ready",
    diagnostics: [],
  },
  {
    id: "c:/claude-manual",
    agent: "claude",
    path: "C:/history/claude/projects",
    canonical_identity: "c:/claude-manual",
    identity_confirmed: true,
    origins: ["手动"],
    mapping_path: null,
    row_enabled: false,
    enabled: false,
    state: "missing",
    diagnostics: [],
  },
  {
    id: "c:/codex-logs",
    agent: "codex",
    path: "C:/codex",
    canonical_identity: "c:/codex-logs",
    identity_confirmed: true,
    origins: ["默认自动"],
    mapping_path: null,
    row_enabled: true,
    enabled: true,
    state: "ready",
    diagnostics: [],
  },
];

/** RC03：可控完成时序的 deferred（按确定顺序释放响应）。 */
function deferred<T>(): [Promise<T>, (v: T) => void] {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return [promise, resolve];
}

/**
 * RC03：未处理拒绝探针。happy-dom 不会把 `unhandledrejection` 派发给 window
 * 监听器（实测只走 Node 的 process 事件），只挂 window 监听等于空断言；
 * 因此同时挂 process 监听，并由用例先做一次自检泄漏证明探针真的能红。
 */
function rejectionProbe(): { reasons: unknown[]; leak: (m: string) => void; stop: () => void } {
  const reasons: unknown[] = [];
  const onNode = (r: unknown): void => {
    reasons.push(r);
  };
  const onWindow = (e: PromiseRejectionEvent): void => {
    reasons.push(e.reason);
  };
  const proc = (
    globalThis as unknown as {
      process?: {
        on: (ev: string, cb: (r: unknown) => void) => void;
        off: (ev: string, cb: (r: unknown) => void) => void;
      };
    }
  ).process;
  window.addEventListener("unhandledrejection", onWindow);
  proc?.on("unhandledRejection", onNode);
  return {
    reasons,
    leak: (m: string) => {
      void Promise.reject(new Error(m));
    },
    stop: () => {
      window.removeEventListener("unhandledrejection", onWindow);
      proc?.off("unhandledRejection", onNode);
    },
  };
}

/** 排空微任务与一个宏任务，让可能的悬挂拒绝被探针记录。 */
async function settleAsync(): Promise<void> {
  await flushPromises();
  await new Promise((r) => setTimeout(r, 0));
  await flushPromises();
}

/** 在指定通知条内找"重试"按钮。 */
function retryButtonFor(w: VueWrapper, noticeText: string) {
  const notice = w.findAll(".ts-notice").find((n) => n.text().includes(noticeText));
  return notice?.findAll("button").find((b) => b.text() === "重试");
}

/** Settings 内部状态（script setup 绑定在开发构建下可经 vm 代理访问）。 */
type SettingsVm = {
  closeAction: string;
  closeActionKnown: boolean;
  autoSync: boolean | null;
  agentEnabled: Record<string, boolean>;
  agentAuto: Record<string, boolean>;
  addDrafts: Record<string, string>;
  pricing: { modelsdev_count: number; entries: unknown[] } | null;
  setCloseAction: (v: string) => Promise<void>;
  setAutoSync: (v: boolean) => Promise<void>;
};

function vmOfSettings(w: VueWrapper): SettingsVm {
  return w.vm as unknown as SettingsVm;
}

function mockBase(): void {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "pricing_entries") return Promise.resolve(pricingView);
    if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
    if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
    if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
    if (cmd === "autostart_status") return Promise.resolve(false);
    if (cmd === "loadAutostart" || cmd === "loadAutoSync") return Promise.resolve(null);
    return Promise.resolve(null);
  });
}

// ── U05：设置页分区布局的可控断点 ─────────────────────────────
// 设置页在窗口 ≤ 1100px 时把左侧分区导航换成顶部分段控件。happy-dom 的
// matchMedia 行为不稳定（也可能缺失），因此测试统一安装可控实现：默认宽
// 窗口，需要时切换并派发 change，形态由 data-nav-mode 可断言。
let layoutNarrow = false;
type MediaListener = (e: { matches: boolean }) => void;
const layoutListeners = new Set<MediaListener>();

function installLayoutMedia(): void {
  layoutNarrow = false;
  layoutListeners.clear();
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: (query: string) => ({
      get matches() {
        return layoutNarrow;
      },
      media: query,
      onchange: null,
      addEventListener: (_t: string, cb: MediaListener) => {
        layoutListeners.add(cb);
      },
      removeEventListener: (_t: string, cb: MediaListener) => {
        layoutListeners.delete(cb);
      },
      addListener: (cb: MediaListener) => {
        layoutListeners.add(cb);
      },
      removeListener: (cb: MediaListener) => {
        layoutListeners.delete(cb);
      },
      dispatchEvent: () => false,
    }),
  });
}

/** U05：切到某个分区（等价于点击导航项）。 */
async function openSection(w: VueWrapper, id: string): Promise<void> {
  (w.vm as unknown as { selectSection: (s: string) => void }).selectSection(id);
  await flushPromises();
}

/** 打开说明浮层并读取其文本（浮层内容只在打开时进入 DOM）。 */
async function openHelpTip(trigger: {
  trigger: (event: string) => Promise<unknown>;
}): Promise<string> {
  await trigger.trigger("focus");
  const deadline = Date.now() + 1000;
  while (Date.now() < deadline) {
    const body = document.querySelector(".help-tooltip-body");
    if (body?.textContent) return body.textContent;
    await flushPromises();
    await new Promise((r) => setTimeout(r, 10));
  }
  return document.querySelector(".help-tooltip-body")?.textContent ?? "";
}

beforeEach(() => {
  invokeMock.mockReset();
  mockBase();
  installLayoutMedia();
  // U06：自动读取间隔是模块级单例（缺省 5 分钟）——每个用例从缺省起步。
  resetCollectIntervalForTests();
});

function settingsImpl(cmd: string): unknown {
  if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
  if (cmd === "pricing_entries") return Promise.resolve(pricingView);
  if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
  if (cmd === "autostart_status") return Promise.resolve(false);
  return Promise.resolve(null);
}

describe("Settings 同步状态传播（Task 4）", () => {
  it("large pricing uses the shared scroll loader instead of manual pagination", async () => {
    // 单测覆盖跨批次接线；8466 条真实渲染规模由浏览器契约覆盖。
    const entries = Array.from({ length: 401 }, (_, i) => ({
      prefix: "model-" + i,
      source: "models.dev",
    }));
    const base = invokeMock.getMockImplementation()!;
    invokeMock.mockImplementation((command: string) =>
      command === "pricing_entries" ? Promise.resolve({ ...pricingView, entries }) : base(command),
    );
    const w = shallowMount(Settings, {
      props: { refreshKey: 0 },
    });
    await flushPromises();
    // U05：价格表只在首次进入「价格」分区时挂载。
    await openSection(w, "pricing");
    const scroll = w.findComponent({ name: "ScrollList" });
    expect(scroll.props("rows")).toHaveLength(401);
    expect(w.find(".pricing-pagination").exists()).toBe(false);
    w.unmount();
  });

  it("进入设置消费后台预读，不重复请求，也没有整页加载遮罩", async () => {
    const preload = createSettingsPreload(invokeMock);
    preload.start();
    await flushPromises();
    const w = mount(Settings, {
      props: { refreshKey: 0 },
      global: { provide: { [SETTINGS_PRELOAD as symbol]: preload } },
    });
    await flushPromises();
    // source_paths_list / cache_stats / pricing_entries / settings_get /
    // autostart_status + H05 的来源库默认路径解析（只解析路径，不打开 CCS 库）
    expect(invokeMock).toHaveBeenCalledTimes(7);
    expect(vmOfSettings(w).autoSync).toBe(true);
    // U05：价格视图推迟到首次进入「价格」分区才消费——仍复用**同一次**预读，
    // 不重复发起 pricing_entries（总调用次数保持 7）。
    expect(vmOfSettings(w).pricing).toBeNull();
    await openSection(w, "pricing");
    expect(vmOfSettings(w).pricing?.modelsdev_count).toBe(10);
    expect(invokeMock).toHaveBeenCalledTimes(7);
    expect(w.findComponent({ name: "NSpin" }).exists()).toBe(false);
    w.unmount();
  });
  it("sync 部分失败仍重新调用 pricing_entries", async () => {
    let syncCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "sync_pricing_openrouter") {
        syncCalls += 1;
        return Promise.reject(new Error("OpenRouter: network down"));
      }
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const entriesBefore = invokeMock.mock.calls.filter((c) => c[0] === "pricing_entries").length;
    const syncBtn = w.findAll("button").find((b) => b.text().includes("同步在线价格"));
    expect(syncBtn).toBeDefined();
    await syncBtn!.trigger("click");
    await flushPromises();
    const entriesAfter = invokeMock.mock.calls.filter((c) => c[0] === "pricing_entries").length;
    expect(syncCalls).toBe(1);
    expect(entriesAfter).toBeGreaterThan(entriesBefore);
  });

  it("同步结束后派发 pricing-status-changed 事件", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "sync_pricing_openrouter") return Promise.resolve([]);
      return Promise.resolve(null);
    });
    const listener = vi.fn();
    window.addEventListener("pricing-status-changed", listener);
    const w = mount(Settings);
    await flushPromises();
    const syncBtn = w.findAll("button").find((b) => b.text().includes("同步在线价格"));
    await syncBtn!.trigger("click");
    await flushPromises();
    expect(listener).toHaveBeenCalled();
    window.removeEventListener("pricing-status-changed", listener);
  });
});

describe("Settings 来源目录管理（PR3）", () => {
  const mountWith = () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    return mount(Settings);
  };

  it("来源列表渲染多标签行与状态（默认自动/手动；前端不重算身份）", async () => {
    const w = await mountWith();
    await flushPromises();
    // 合并行/多标签：手动行展示「手动」标签与完整路径
    expect(w.text()).toContain("默认自动");
    expect(w.text()).toContain("手动");
    expect(w.text()).toContain("C:/history/claude/projects");
    // 停用行显示"已停用 · 缺失"
    expect(w.text()).toContain("已停用 · 缺失");
    // 无重新发现按钮
    expect(w.text()).not.toContain("重新发现");
    // 环境重启提示
    expect(w.text()).toContain("修改后需重启");
    w.unmount();
  });

  it("添加成功：草稿清空、列表重读（source_paths_list 再次调用）", async () => {
    const w = await mountWith();
    await flushPromises();
    const callsBefore = invokeMock.mock.calls.filter((c) => c[0] === "source_paths_list").length;
    const addInput = w.find("input#source-add-claude");
    await addInput.setValue("D:/history/claude/projects");
    const addBtn = w
      .findAll("button")
      .find((b) => b.attributes("aria-label") === "Claude Code 添加目录");
    await addBtn!.trigger("click");
    await flushPromises();
    expect(invokeMock).toHaveBeenCalledWith("source_path_add", {
      agent: "claude",
      path: "D:/history/claude/projects",
    });
    const callsAfter = invokeMock.mock.calls.filter((c) => c[0] === "source_paths_list").length;
    expect(callsAfter).toBeGreaterThan(callsBefore);
    const vm = w.vm as unknown as { addDrafts: Record<string, string> };
    expect(vm.addDrafts.claude, "成功后草稿清空").toBe("");
    w.unmount();
  });

  it("添加被拒绝（父/子冲突）：错误可见且草稿保留", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "source_path_add") return Promise.reject(new Error("目录与已有来源存在嵌套关系"));
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const addInput = w.find("input#source-add-claude");
    await addInput.setValue("C:/claude-logs/sub");
    const addBtn = w
      .findAll("button")
      .find((b) => b.attributes("aria-label") === "Claude Code 添加目录");
    await addBtn!.trigger("click");
    await flushPromises();
    const errText = "目录与已有来源存在嵌套关系";
    expect(w.text().split(errText).length - 1).toBe(1);
    const vm = w.vm as unknown as { addDrafts: Record<string, string> };
    expect(vm.addDrafts.claude, "失败保留草稿").toBe("C:/claude-logs/sub");
    w.unmount();
  });

  it("移除只对手动行可用；default 行无移除按钮", async () => {
    const w = await mountWith();
    await flushPromises();
    const removeButtons = w
      .findAll("button")
      .filter((b) => b.attributes("aria-label")?.startsWith("移除 "));
    expect(removeButtons.length, "只有手动行可移除").toBe(1);
    expect(removeButtons[0].attributes("aria-label")).toContain("C:/history/claude/projects");
    w.unmount();
  });

  function agentBlock(w: ReturnType<typeof mount>, agent: string) {
    const strong = w
      .findAll("strong")
      .find((x) => x.text() === (agent === "claude" ? "Claude Code" : "Codex"));
    expect(strong, `${agent} 块应存在`).toBeDefined();
    return strong!.element.parentElement!.parentElement!;
  }

  it("行启停：invoke source_path_set_enabled；失败回滚显示值", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "source_path_set_enabled") return Promise.reject(new Error("来源已变化，请重试"));
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = w.vm as unknown as {
      sourceRows: { id: string; row_enabled: boolean }[];
    };
    const disabledRow = vm.sourceRows.find((r) => !r.row_enabled)!;
    disabledRow.row_enabled = false;
    // 通过组件方法触发：直接找停用行的 switch（stub 后为 div[role=switch]）
    // 作用域限定在 claude 块：块内第 3 个 role=switch 是手动行开关
    const block = agentBlock(w, "claude");
    const switches = Array.from(block.querySelectorAll('[role="switch"]'));
    expect(switches.length, "claude 块：总开关 + 自动开关 + 行开关（每行一个）").toBe(
      2 + rowsOfLength(w, "claude"),
    );

    function rowsOfLength(w: ReturnType<typeof mount>, agent: string): number {
      const vm = w.vm as unknown as { sourceRows: { agent: string }[] };
      return vm.sourceRows.filter((r) => r.agent === agent).length;
    }
    switches[switches.length - 1]!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await flushPromises();
    expect(
      invokeMock.mock.calls.some(
        (c) => c[0] === "source_path_set_enabled" && c[1].enabled === true,
      ),
      "停用行的开关点击 = 启用",
    ).toBe(true);
    expect(vm.sourceRows.find((r) => r.id === "c:/claude-manual")!.row_enabled, "失败回滚").toBe(
      false,
    );
    w.unmount();
  });

  it("总开关与自动开关即改即存（source_config_set / source_auto_set）", async () => {
    const w = await mountWith();
    await flushPromises();
    const block = agentBlock(w, "claude");
    const switches = Array.from(block.querySelectorAll('[role="switch"]'));
    const click = (el: Element) => el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    click(switches[0]!); // claude 总开关 → false
    await flushPromises();
    expect(invokeMock).toHaveBeenCalledWith("source_config_set", {
      agent: "claude",
      enabled: false,
      dir: null,
    });
    click(switches[1]!); // claude 自动开关 → false
    await flushPromises();
    expect(invokeMock).toHaveBeenCalledWith("source_auto_set", {
      agent: "claude",
      enabled: false,
    });
    w.unmount();
  });
});

import type { PricingEntry } from "../types";
import {
  fmtPriceOrUnknown,
  formatRates,
  formatSchedule,
  formatSegmentRange,
  formatTieredPricing,
} from "../lib/tieredPrice";

describe("Settings 分段价格展示（Task 8）", () => {
  const tiered: PricingEntry = {
    prefix: "nano-gpt/qwen/tiered-view",
    name: "Tiered View",
    channel: "nano-gpt",
    input: 4.0,
    output: 20.0,
    cache_write: 0.0,
    cache_read: null,
    source: "外置",
    base_incomplete: true,
    basis: "prompt_tokens",
    has_tiered_pricing: true,
    segments: [
      {
        label: ">272K",
        min_tokens: 272001,
        max_tokens: null,
        prices: { input: 8.0, output: 30.0, cache_write: null, cache_read: null },
      },
    ],
    schedules: [
      {
        label: "peak",
        timezone: "UTC",
        periods: [
          {
            start_time: "12:00",
            end_time: "14:00",
            weekdays: ["mon", "fri"],
            prices: { input: 30.0 },
          },
        ],
      },
    ],
  };
  const plain: PricingEntry = {
    prefix: "plain-model",
    input: 1.0,
    output: 2.0,
    cache_write: 0.25,
    cache_read: 0.02,
    source: "models.dev",
    has_tiered_pricing: false,
  };

  it("缺失分项显示未知，显式 0 显示 $0", () => {
    expect(fmtPriceOrUnknown(null)).toBe("未知");
    expect(fmtPriceOrUnknown(0)).toBe("$0.00");
    // RC07：单价保留最短可往返有效表示（不再强制四位/两位）。
    expect(fmtPriceOrUnknown(0.4)).toBe("$0.4");
    expect(fmtPriceOrUnknown(8)).toBe("$8");
  });

  it("分段范围与单价行展开；未知分项可见", () => {
    expect(formatSegmentRange(tiered.segments![0])).toBe(">272K：[272,001, ∞)");
    expect(formatRates(tiered.segments![0].prices)).toContain("输入 $8");
    expect(formatRates(tiered.segments![0].prices)).toContain("缓存命中 未知");
  });

  it("峰谷规则展示标签、时区、时段与星期限制", () => {
    const line = formatSchedule(tiered.schedules![0]);
    expect(line).toContain("peak（UTC）");
    expect(line).toContain("12:00–14:00");
    expect(line).toContain("mon,fri");
  });

  it("档位展开包含依据/分段/峰谷；普通条目为空保持现有布局", () => {
    const lines = formatTieredPricing(tiered);
    expect(lines.some((l) => l.includes("计价依据：prompt_tokens"))).toBe(true);
    expect(lines.some((l) => l.includes("分段 >272K"))).toBe(true);
    expect(lines.some((l) => l.includes("峰谷 peak"))).toBe(true);
    expect(formatTieredPricing(plain)).toEqual([]);
  });
});

describe("Settings 分组与状态（设计系统 Task 7）", () => {
  it("设置页按 通用/数据源/用量数据/价格/外观/高级 分区渲染（U05）", async () => {
    const w = mount(Settings);
    await flushPromises();
    const titles = w.findAll(".group-title").map((t) => t.text());
    for (const title of ["通用", "数据源", "用量数据", "价格", "外观", "高级"]) {
      expect(titles, `分区 ${title} 应存在`).toContain(title);
    }
    // U05：旧「缓存」计数标签不再出现（改名「已采集文件 / 已保存请求」）
    expect(w.text()).not.toContain("缓存文件");
    expect(w.text()).not.toContain("缓存事件");
  });

  it("计价规则收进说明浮层：页面只留结论，规则原文完整可读（N08/R03/U05）", async () => {
    const w = mount(Settings, { attachTo: document.body });
    await flushPromises();
    await openSection(w, "pricing");
    // U05：长说明不再铺在页面上，只在浮层里给出。
    const trigger = w.find("button.help-trigger[aria-label='价格候选选择规则']");
    expect(trigger.exists(), "候选规则说明触发器应存在").toBe(true);
    const body = await openHelpTip(trigger);
    expect(body, "浮层必须给出完整规则").toContain("有完整四桶价格的候选优先");
    expect(body).toContain("没有完整候选时才回退到部分候选中已知费用最高者");
    expect(body).not.toContain("全部候选按本次请求条件计价");
    expect(w.text(), "页面不重复铺长说明").not.toContain("没有完整候选时才回退到部分候选中");
  });

  it("来源身份标签用中性色（成功/警告只表达同步状态）", async () => {
    const w = mount(Settings);
    await flushPromises();
    // pricingView.entries 为空 → 无来源行；断言不出现成功/警告色的身份标签
    const tags = w.findAll(".n-tag");
    const identity = tags.filter((t) => ["外置", "models.dev", "OpenRouter"].includes(t.text()));
    for (const t of identity) {
      expect(
        t.classes().some((c) => c.includes("success") || c.includes("warning")),
        `身份标签 ${t.text()} 不得使用状态色`,
      ).toBe(false);
    }
  });

  it("技术路径与同步时间收纳到可展开区域", async () => {
    const w = mount(Settings);
    await flushPromises();
    // naive-ui mock 的第二个工厂覆盖了 stub——直接断言真实 NCollapse DOM
    const item = w.find(".n-collapse-item");
    expect(item.exists(), "应存在可展开区域").toBe(true);
    expect(w.text()).toContain("技术详情");
  });
});

describe("Settings macOS 分组结构（设计系统 Task 7）", () => {
  it("每个分区一张 .ts-card，分区标题（.group-title）在卡片外", async () => {
    const w = mount(Settings);
    await flushPromises();
    // U05：一次只显示一个分区——DOM 顺序即分区顺序（未选中的用 v-show 保留草稿）。
    const titles = w.findAll(".group-title").map((t) => t.text());
    expect(titles).toEqual(["通用", "数据源", "用量数据", "价格", "外观", "高级"]);
    const cards = w.findAll("section.ts-card");
    expect(cards.length).toBe(7);
    for (const t of w.findAll(".group-title")) {
      expect(t.element.closest("section.ts-card"), "组标题必须在卡片外").toBeNull();
    }
  });

  it("设置项为左标签右控件行（.setting-row），行间发丝线", async () => {
    const w = mount(Settings);
    await flushPromises();
    const rows = w.findAll(".setting-row");
    // 至少：时区、自启、已采集文件、已保存请求、自动同步价格
    expect(rows.length).toBeGreaterThanOrEqual(5);
    const tzRow = rows.find((r) => r.text().includes("聚合/展示时区"));
    expect(tzRow).toBeDefined();
    expect(tzRow!.find(".setting-label").exists()).toBe(true);
    // 行控件在右侧容器
    expect(tzRow!.find(".setting-control").exists()).toBe(true);
  });

  it("主源未就绪与快照警告渲染为 .ts-notice（不再使用 NAlert）", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_entries")
        return Promise.resolve({
          ...pricingView,
          modelsdev_count: 0,
          modelsdev_synced_at: null,
          warnings: ["OpenRouter 快照损坏，已跳过 3 条"],
        });
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    await openSection(w, "pricing");
    const notices = w.findAll(".ts-notice");
    expect(notices.some((n) => n.text().includes("主源（models.dev）尚未就绪"))).toBe(true);
    expect(notices.some((n) => n.text().includes("OpenRouter 快照损坏"))).toBe(true);
    expect(w.find(".n-alert").exists()).toBe(false);
  });
});

describe("Settings 关闭行为与高级配置（关闭确认与配置文件计划 Task 4）", () => {
  beforeEach(() => {
    msgSpy.success.mockClear();
    msgSpy.error.mockClear();
    msgSpy.info.mockClear();
  });

  function vmOf(w: ReturnType<typeof mount>) {
    return w.vm as unknown as {
      closeAction: string;
      setCloseAction: (v: string) => Promise<void>;
    };
  }

  it("关闭窗口时默认每次询问；选最小化/退出写盘，恢复询问传 null", async () => {
    const w = mount(Settings);
    await flushPromises();
    expect(w.text()).toContain("关闭窗口时");
    const vm = vmOf(w);
    expect(vm.closeAction).toBe("ask");
    await vm.setCloseAction("minimize");
    expect(invokeMock).toHaveBeenCalledWith("settings_set_close_action", { action: "minimize" });
    await vm.setCloseAction("quit");
    expect(invokeMock).toHaveBeenCalledWith("settings_set_close_action", { action: "quit" });
    await vm.setCloseAction("ask");
    expect(invokeMock).toHaveBeenCalledWith("settings_set_close_action", { action: null });
    expect(vm.closeAction).toBe("ask");
  });

  it("记忆过的默认动作从 settings_get 读取显示", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, sources: {}, close_action: "quit" });
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    expect(vmOf(w).closeAction).toBe("quit");
  });

  it("高级分区：底部标题 + 打开设置配置文件按钮调用 open_settings_file", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "open_settings_file")
        return Promise.resolve("C:/Users/x/.tokenscope/settings.toml");
      return mockBaseImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const titles = w.findAll(".group-title").map((t) => t.text());
    expect(titles.at(-1)).toBe("高级");
    expect(w.text()).toContain("settings.toml");
    const btn = w.findAll("button").find((b) => b.text().includes("打开设置配置文件"));
    expect(btn).toBeDefined();
    await btn!.trigger("click");
    await flushPromises();
    expect(invokeMock).toHaveBeenCalledWith("open_settings_file");
    expect(msgSpy.info).toHaveBeenCalled();
  });

  /** mockBase 的命令分发表（供个别用例覆盖特定命令时兜底）。 */
  function mockBaseImpl(cmd: string): unknown {
    if (cmd === "pricing_entries") return Promise.resolve(pricingView);
    if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
    if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
    if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
    return Promise.resolve(null);
  }
});

// ── UX06：设置首载与定价横幅错误恢复 ──
// 各读取区块独立失败 + 局部重试；重试不覆盖已编辑草稿；晚到响应不覆盖
// 已保存值；保存成功后状态刷新失败与保存失败区分。
describe("Settings 首载错误恢复（UX06）", () => {
  it("settings_initial_failures_are_independent_and_retryable：单区块失败不影响其他区块，错误可见可重试", async () => {
    // source_status 拒绝，其余成功。
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_paths_list") return Promise.reject(new Error("boom-status"));
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    // 失败区块：错误可见 + 重试按钮。
    expect(w.text()).toContain("来源状态读取失败");
    expect(w.text()).toContain("boom-status");
    // 其他成功区块照常显示（用量数据统计、价格表）。
    expect(w.text()).toContain("已采集文件");
    expect(w.text()).toContain("同步在线价格");
    // 重试成功后错误清除。
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      return Promise.resolve(null);
    });
    const retry = w.findAll("button").find((b) => b.text() === "重试");
    expect(retry).toBeDefined();
    await retry!.trigger("click");
    await flushPromises();
    expect(w.text()).not.toContain("来源状态读取失败");
    w.unmount();
  });

  it("settings_get 失败：开关显示缺省值且来源列表照常渲染；恢复后重试", async () => {
    let settingsFail = true;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        return settingsFail
          ? Promise.reject(new Error("boom-settings"))
          : Promise.resolve({ price_auto_sync: true, sources: {} });
      }
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    // 设置读取失败：开关显示缺省（true），但来源列表（独立区块）照常渲染。
    expect(w.text()).toContain("设置读取失败");
    const vm = w.vm as unknown as { agentEnabled: Record<string, boolean> };
    expect(vm.agentEnabled.claude).toBe(true);
    expect(w.text()).toContain("C:/claude-logs");
    // 恢复后重试：开关采用落盘值。
    settingsFail = false;
    const retry = w.findAll("button").find((b) => b.text() === "重试");
    await retry!.trigger("click");
    await flushPromises();
    expect((w.vm as unknown as { settingsError: string | null }).settingsError).toBeNull();
    w.unmount();
  });

  it("retry_preserves_add_draft：区块重试不清空未提交的追加草稿", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") {
        return Promise.resolve({
          price_auto_sync: true,
          sources: { claude: { enabled: false, auto_discover: false } },
        });
      }
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = w.vm as unknown as {
      addDrafts: Record<string, string>;
      agentEnabled: Record<string, boolean>;
    };
    // 用户输入了未提交的追加草稿
    vm.addDrafts.claude = "D:/user-typing";
    // 区块重试（loadSettings + loadSources）
    await (w.vm as unknown as { loadSettings: () => Promise<void> }).loadSettings();
    await (w.vm as unknown as { loadSources: () => Promise<void> }).loadSources();
    await flushPromises();
    // 草稿保留；开关取落盘值
    expect(vm.addDrafts.claude).toBe("D:/user-typing");
    expect(vm.agentEnabled.claude).toBe(false);
    w.unmount();
  });

  it("late_settings_response_does_not_overwrite_saved_agent_enabled：晚到读取不覆盖已保存开关", async () => {
    let release!: (v: unknown) => void;
    const gate = new Promise((r) => {
      release = r;
    });
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        return cmd === "settings_get" && gate
          ? gate.then(() => ({ price_auto_sync: true, sources: {} }))
          : Promise.resolve({ price_auto_sync: true, sources: {} });
      }
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "source_config_set") return Promise.resolve(null);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    // 用户切换总开关（保存完成）；随后晚到的旧设置读取不得覆盖。
    const vm = w.vm as unknown as {
      agentEnabled: Record<string, boolean>;
      setAgentEnabled: (a: "claude", v: boolean) => void;
    };
    vm.setAgentEnabled("claude", false);
    await flushPromises();
    expect(vm.agentEnabled.claude).toBe(false);
    expect(invokeMock).toHaveBeenCalledWith("source_config_set", {
      agent: "claude",
      enabled: false,
      dir: null,
    });
    release(null);
    await flushPromises();
    expect(vm.agentEnabled.claude, "晚到读取不得覆盖保存值").toBe(false);
    w.unmount();
  });

  it("追加保存成功但列表刷新失败：区分提示（不误导重复保存）", async () => {
    let addedOnce = false;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_path_add") {
        addedOnce = true;
        return Promise.resolve("added:new-id");
      }
      if (cmd === "source_paths_list") {
        return addedOnce
          ? Promise.reject(new Error("refresh down"))
          : Promise.resolve(sourceRowsFixture);
      }
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const addInput = w.find("input#source-add-claude");
    expect(addInput.exists(), "claude 追加输入应存在").toBe(true);
    await addInput.setValue("D:/history/claude/projects");
    const addBtn = w
      .findAll("button")
      .find((b) => b.attributes("aria-label") === "Claude Code 添加目录");
    await addBtn!.trigger("click");
    await flushPromises();
    // RC03：提示必须保留真实失败原因，不能只说"可重试"。
    expect(w.text()).toContain("已保存，列表刷新失败");
    expect(w.text()).toContain("refresh down");
    expect(w.text()).toContain("可重试");
    w.unmount();
  });

  it("syncPricing 后价格列表刷新失败：归类价格列表读取失败并保留旧数据", async () => {
    let synced = false;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "sync_pricing_openrouter") {
        synced = true;
        return Promise.resolve([{ source: "models.dev", count: 5 }]);
      }
      if (cmd === "pricing_entries") {
        return synced ? Promise.reject(new Error("list down")) : Promise.resolve(pricingView);
      }
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const sync = w.findAll("button").find((b) => b.text().includes("同步在线价格"));
    await sync!.trigger("click");
    await flushPromises();
    expect(w.text()).toContain("价格列表读取失败");
    // syncing 退出（按钮不再 loading）。
    const sync2 = w.findAll("button").find((b) => b.text().includes("同步在线价格"));
    expect((sync2!.element as HTMLButtonElement).classList.toString()).not.toContain(
      "n-button--loading",
    );
    w.unmount();
  });
});

// UX08：设置页层次、目录信息与有限清理。
describe("Settings 目录信息与重建（UX08）", () => {
  it("source_directory_input_has_stable_name：追加输入有稳定可访问名称且与 label 关联", async () => {
    const w = mount(Settings);
    await flushPromises();
    const inputs = w.findAll(".n-input__input-el");
    expect(inputs.map((i) => i.attributes("aria-label"))).toEqual([
      "Claude Code 追加日志目录",
      "Codex 追加日志目录",
      // H05：CCS 导入的来源库路径同样给稳定可访问名称
      "来源库路径",
    ]);
    // label[for] 指向真实 input 的 id（名称不靠 placeholder 兜底）
    for (const input of inputs) {
      const id = input.attributes("id");
      expect(id, "真实 input 必须有稳定 id").toBeTruthy();
      const label = w.find(`label[for="${id}"]`);
      expect(label.exists()).toBe(true);
      expect(label.text()).toBe(input.attributes("aria-label"));
    }
  });
  it("row_paths_are_available_in_full：来源行完整路径可读、可复制、长路径换行", async () => {
    const w = mount(Settings);
    await flushPromises();
    const paths = w.findAll(".source-effective .effective-path");
    expect(paths.map((p) => p.text())).toEqual([
      "C:/claude-logs",
      "C:/history/claude/projects",
      "C:/codex",
    ]);
    // 长路径换行不省略（word-break 断词；源文本契约锁定类与内容来源）
    const styles = /\.effective-path\s*\{([^}]*)\}/.exec(settingsSource);
    expect(styles?.[1] ?? "").toContain("word-break");
    w.unmount();
  });

  it("rebuild_keeps_progress_and_prevents_repeat：重建有预期说明、进度保留、不重复触发", async () => {
    let resolveRebuild!: (v: unknown) => void;
    let calls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "refresh_cache") {
        calls += 1;
        return new Promise((r) => {
          resolveRebuild = r;
        });
      }
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    msgSpy.success.mockClear();
    const w = mount(Settings);
    await flushPromises();
    // 预期说明可见
    expect(w.text()).toContain("重新扫描日志，可能需要一段时间");
    const btn = w.findAll("button").find((b) => b.text().includes("重扫日志"))!;
    expect(btn).toBeDefined();
    await btn.trigger("click");
    // 进行中：按钮处于 loading（进度反馈保留）
    expect(btn.classes()).toContain("n-button--loading");
    await btn.trigger("click"); // 第二次必须被函数级 guard 拒绝
    expect(calls).toBe(1);
    resolveRebuild({ path: "p", files: 5, events: 9 });
    await flushPromises();
    expect(calls).toBe(1);
    // H04/AP03：成功提示必须说明范围（按当前来源配置重新解析）与"历史用量
    // 不受影响"，不能只说"已重建"让用户以为配置外来源也被采集。
    expect(msgSpy.success).toHaveBeenCalledWith(
      "已重扫日志（按当前来源配置重新解析，已保存的历史用量不受影响）",
    );
    expect(btn.classes()).not.toContain("n-button--loading");
  });

  it("save_preserves_existing_error_states：追加失败不丢失其他已有错误", async () => {
    invokeMock.mockImplementation((cmd: string, args?: { path?: string }) => {
      if (cmd === "pricing_entries") return Promise.reject(new Error("价格读取失败"));
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      // codex 追加失败 → 该来源错误；claude 追加成功
      if (cmd === "source_path_add") {
        return args?.path === "C:/dup"
          ? Promise.reject(new Error("来源目录与 Claude Code 重叠"))
          : Promise.resolve("added:ok");
      }
      return Promise.resolve(null);
    });
    msgSpy.success.mockClear();
    const w = mount(Settings);
    await flushPromises();
    // 先制造价格区块错误（U05：需先进入分区）
    await openSection(w, "pricing");
    expect(w.text()).toContain("价格列表读取失败");
    // codex 追加失败 → 该来源错误
    const codexInput = w.find("input#source-add-codex");
    await codexInput.setValue("C:/dup");
    const codexAdd = w
      .findAll("button")
      .find((b) => b.attributes("aria-label") === "Codex 添加目录")!;
    await codexAdd.trigger("click");
    await flushPromises();
    expect(w.text()).toContain("来源目录与 Claude Code 重叠");

    // claude 追加成功（价格失败保留、codex 错误清除）
    const claudeInput = w.find("input#source-add-claude");
    await claudeInput.setValue("D:/new");
    const claudeAdd = w
      .findAll("button")
      .find((b) => b.attributes("aria-label") === "Claude Code 添加目录")!;
    await claudeAdd.trigger("click");
    await flushPromises();
    expect(msgSpy.success).not.toHaveBeenCalled(); // 添加不弹全局通知
    expect(w.text()).toContain("价格列表读取失败");
    // codex 自己的错误保留（claude 的成功不清别人的错误）
    expect(w.text()).toContain("来源目录与 Claude Code 重叠");
    w.unmount();
  });
  it("settings_initial_load_reads_once：进入设置页只读一次设置", async () => {
    let settingsCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        settingsCalls += 1;
        return Promise.resolve({
          price_auto_sync: true,
          close_action: "quit",
          sources: {
            claude: { enabled: true, dir: "C:/c" },
            codex: { enabled: false, dir: "D:/x" },
          },
        });
      }
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    expect(settingsCalls, "首载只允许一次 settings_get").toBe(1);
    const vm = vmOfSettings(w);
    // 三处依赖都由这一次读取填充
    expect(vm.closeAction).toBe("quit");
    expect(vm.closeActionKnown).toBe(true);
    expect(vm.autoSync).toBe(true);
    expect(vm.agentEnabled.claude).toBe(true);
    expect(vm.agentEnabled.codex).toBe(false);
    w.unmount();
  });

  it("settings_initial_failures_stay_isolated_and_raise_no_unhandled_rejection", async () => {
    const probe = rejectionProbe();
    // 自检：先制造一条真实泄漏，确认探针**能够**变红——否则"无未处理拒绝"
    // 只是空断言（happy-dom 下 window 监听器永远收不到）。
    probe.leak("probe-sentinel");
    await settleAsync();
    expect(
      probe.reasons.some((r) => String(r).includes("probe-sentinel")),
      "探针必须能捕捉未处理拒绝",
    ).toBe(true);
    probe.reasons.length = 0;

    const cases: [string, string][] = [
      ["settings_get", "设置读取失败，来源配置暂不可保存"],
      ["source_paths_list", "来源状态读取失败"],
      ["cache_stats", "用量数据统计读取失败"],
      ["pricing_entries", "价格列表读取失败"],
      ["autostart_status", "自启状态读取失败"],
    ];
    for (const [failing, expected] of cases) {
      invokeMock.mockReset();
      invokeMock.mockImplementation((cmd: string) =>
        cmd === failing ? Promise.reject(new Error(`${failing} down`)) : settingsImpl(cmd),
      );
      const w = mount(Settings);
      await settleAsync();
      // U05：价格视图推迟到首次进入「价格」分区才读取（错误条随之出现）。
      if (failing === "pricing_entries") await openSection(w, "pricing");
      expect(w.text(), failing).toContain(expected);
      expect(w.text(), failing).toContain(`${failing} down`);
      // 其他成功区块照常渲染（用量数据/价格/数据源三组都在）
      expect(w.text(), failing).toContain("已采集文件");
      expect(w.text(), failing).toContain("同步在线价格");
      expect(w.text(), failing).toContain("数据源");
      expect(probe.reasons, failing).toEqual([]);
      w.unmount();
    }
    probe.stop();
  });

  it("block_retry_never_starts_a_network_sync：任何读取重试都只重读，不触发同步", async () => {
    // RC03/RC04：读取重试的语义是"重新读取该区块"，不得顺手发起联网同步。
    let fail = true;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get" && fail) return Promise.reject(new Error("设置读取失败"));
      if (cmd === "source_paths_list" && fail) return Promise.reject(new Error("来源状态读取失败"));
      if (cmd === "cache_stats" && fail) return Promise.reject(new Error("用量数据统计读取失败"));
      if (cmd === "pricing_entries" && fail) return Promise.reject(new Error("价格列表读取失败"));
      if (cmd === "autostart_status" && fail) return Promise.reject(new Error("自启状态读取失败"));
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, close_action: null, sources: {} });
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    // U05：价格区块的错误条只在首次进入「价格」分区后出现（此时仍处失败态）。
    await openSection(w, "pricing");
    fail = false;
    for (const notice of [
      "设置读取失败",
      "来源状态读取失败",
      "用量数据统计读取失败",
      "价格列表读取失败",
      "自启状态读取失败",
    ]) {
      const retry = retryButtonFor(w, notice);
      expect(retry, `${notice} 必须有重试按钮`).toBeDefined();
      await retry!.trigger("click");
      await flushPromises();
    }
    const cmds = invokeMock.mock.calls.map((c) => c[0]);
    expect(cmds, "重试不得触发 sync_pricing_openrouter").not.toContain("sync_pricing_openrouter");
    // 五个区块都恢复正常
    expect(w.text()).not.toContain("读取失败");
    w.unmount();
  });

  it("block_retry_while_pending_rereads_after_and_never_lets_stale_win：在途重试补读一次", async () => {
    // RC03：区块重试防重复，但刷新意图不能丢——在途期间的再次触发本轮
    // 结束后必须补读一次，最终采用新数据，旧响应不得覆盖。
    const [staleRead, releaseStale] = deferred<unknown>();
    let priceCalls = 0;
    const oldView = { ...pricingView, modelsdev_count: 1, entries: [] };
    const newView = { ...pricingView, modelsdev_count: 999, entries: [] };
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_entries") {
        priceCalls += 1;
        return priceCalls === 1 ? staleRead : Promise.resolve(newView);
      }
      if (cmd === "settings_get")
        return Promise.resolve({ ...oldView, price_auto_sync: true, sources: {} });
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w);
    // U05：价格读取由首次进入「价格」分区触发（本条第一次调用挂起）。
    await openSection(w, "pricing");
    expect(vm.pricing, "读取挂起期间不得提前提交价格数据").toBeNull();
    // 首载读取仍挂起时触发刷新（refreshKey）→ 不得并发第二条请求
    await w.setProps({ refreshKey: 1 });
    await flushPromises();
    expect(priceCalls, "在途期间重复刷新不得并发请求").toBe(1);
    // 释放旧读取（内容是刷新前的旧状态）→ 结束后补读一次并采用新值
    releaseStale(oldView);
    await flushPromises();
    expect(priceCalls, "刷新意图必须补读一次").toBe(2);
    expect(vm.pricing?.modelsdev_count, "旧结果不得覆盖补读结果").toBe(999);
    w.unmount();
  });

  it("settings_retry_restores_all_dependent_controls：重试恢复来源/关闭动作/自动同步", async () => {
    let fail = true;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        if (fail) return Promise.reject(new Error("设置读取失败"));
        return Promise.resolve({
          price_auto_sync: true,
          close_action: "quit",
          sources: {
            claude: { enabled: true, dir: "C:/restored" },
            codex: { enabled: false, dir: "D:/restored" },
          },
        });
      }
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w);
    // 失败态：关闭动作未知（不是可操作的"每次询问"），自动同步未知
    expect(vm.closeActionKnown).toBe(false);
    expect(vm.autoSync).toBeNull();
    expect(w.text()).toContain("关闭动作未知");
    // 未知状态不得写入
    await vm.setCloseAction("quit");
    await vm.setAutoSync(true);
    expect(invokeMock.mock.calls.some((c) => c[0] === "settings_set_close_action")).toBe(false);
    expect(invokeMock.mock.calls.some((c) => c[0] === "settings_set_price_auto_sync")).toBe(false);
    // 开关显示缺省值（读取失败 = 未知），但行列表（独立区块）照常渲染
    expect(vm.agentEnabled.claude).toBe(true);
    expect(w.text()).toContain("C:/claude-logs");

    // 重试：三处状态全部恢复
    fail = false;
    const retry = retryButtonFor(w, "设置读取失败");
    expect(retry, "设置错误条必须有重试按钮").toBeDefined();
    await retry!.trigger("click");
    await flushPromises();
    expect(vm.closeAction).toBe("quit");
    expect(vm.closeActionKnown).toBe(true);
    expect(vm.autoSync).toBe(true);
    expect(vm.agentEnabled.claude).toBe(true);
    expect(vm.agentEnabled.codex).toBe(false);
    expect(w.text()).not.toContain("设置读取失败");
    expect(w.text()).not.toContain("关闭动作未知");
    // 恢复后：行列表照常渲染
    expect(w.text()).toContain("C:/claude-logs");
    w.unmount();
  });

  it("add_draft_survives_settings_rereads：设置重读不清空追加草稿", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = w.vm as unknown as { addDrafts: Record<string, string> };
    vm.addDrafts.codex = "D:/pending";
    await (w.vm as unknown as { loadSettings: () => Promise<void> }).loadSettings();
    await flushPromises();
    expect(vm.addDrafts.codex, "重读不得清空草稿").toBe("D:/pending");
    w.unmount();
  });

  it("late_read_after_unmount_is_ignored：卸载后的晚到读取不写状态", async () => {
    const [pending, resolvePending] = deferred<unknown>();
    let n = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        n += 1;
        return n === 1 ? Promise.reject(new Error("首次读取失败")) : pending;
      }
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w);
    const retry = retryButtonFor(w, "设置读取失败");
    await retry!.trigger("click");
    await flushPromises();
    expect(n).toBe(2);
    w.unmount();
    resolvePending({
      price_auto_sync: true,
      close_action: "quit",
      sources: { claude: { enabled: true, dir: "C:/late" }, codex: { enabled: true, dir: "" } },
    });
    await flushPromises();
    expect(vm.closeActionKnown, "卸载后的晚到读取不得写状态").toBe(false);
    expect(vm.addDrafts?.claude ?? "").toBe("");
  });

  it("late_settings_response_does_not_overwrite_saved_close_action：晚到读取不覆盖已保存关闭动作", async () => {
    // RC03：关闭动作也必须是真实 deferred 竞态——首次读取成功 → 第二次读取
    // 挂起 → 用户改关闭动作并写盘成功 → 释放旧读取（最小化）→ 仍是退出。
    const [pendingRead, resolvePendingRead] = deferred<unknown>();
    let settingsCalls = 0;
    const settingsOf = (close: string) => ({
      price_auto_sync: true,
      close_action: close,
      sources: {},
    });
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        settingsCalls += 1;
        return settingsCalls === 2 ? pendingRead : Promise.resolve(settingsOf("ask"));
      }
      if (cmd === "settings_set_close_action") return Promise.resolve("quit");
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w);
    expect(vm.closeAction).toBe("ask");
    expect(vm.closeActionKnown).toBe(true);

    // 第二次读取挂起（内容将是旧值"最小化"）
    await w.setProps({ refreshKey: 1 });
    await flushPromises();
    expect(settingsCalls).toBe(2);
    // 挂起期间用户改关闭动作并成功写盘
    await vm.setCloseAction("quit");
    expect(invokeMock).toHaveBeenCalledWith("settings_set_close_action", { action: "quit" });
    expect(vm.closeAction).toBe("quit");
    expect(w.text()).toContain("直接退出");
    // 释放旧读取
    resolvePendingRead(settingsOf("minimize"));
    await flushPromises();
    expect(vm.closeAction, "晚到读取不得把已保存的退出改回最小化").toBe("quit");
    expect(w.text()).toContain("直接退出");
    expect(w.text()).not.toContain("最小化到托盘");
    w.unmount();
  });

  it("late_settings_response_does_not_overwrite_saved_auto_sync：晚到读取不覆盖已保存自动同步", async () => {
    // RC03：自动同步同样覆盖——保存 false 后释放"true"的旧读取，界面与状态
    // 都必须停在 false。
    const [pendingRead, resolvePendingRead] = deferred<unknown>();
    let settingsCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        settingsCalls += 1;
        return settingsCalls === 2
          ? pendingRead
          : Promise.resolve({ price_auto_sync: true, close_action: null, sources: {} });
      }
      if (cmd === "settings_set_price_auto_sync") return Promise.resolve(false);
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w);
    expect(vm.autoSync).toBe(true);

    await w.setProps({ refreshKey: 1 });
    await flushPromises();
    expect(settingsCalls).toBe(2);
    await vm.setAutoSync(false);
    expect(invokeMock).toHaveBeenCalledWith("settings_set_price_auto_sync", { enabled: false });
    expect(vm.autoSync).toBe(false);
    const sw = w.find('[role="switch"][aria-label="自动同步价格"]');
    expect(sw.exists(), "自动同步开关必须可定位").toBe(true);
    expect(sw.attributes("aria-checked"), "界面开关显示已关闭").toBe("false");

    resolvePendingRead({ price_auto_sync: true, close_action: null, sources: {} });
    await flushPromises();
    expect(vm.autoSync, "晚到读取不得把已保存的关闭改回开启").toBe(false);
    expect(
      w.find('[role="switch"][aria-label="自动同步价格"]').attributes("aria-checked"),
      "界面开关不得被旧读取翻回 true",
    ).toBe("false");
    w.unmount();
  });

  it("add_requests_serialize_per_agent：同一来源的追加请求串行", async () => {
    let inFlight = 0;
    let maxConcurrent = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_path_add") {
        inFlight += 1;
        maxConcurrent = Math.max(maxConcurrent, inFlight);
        return new Promise((r) => {
          setTimeout(() => {
            inFlight -= 1;
            r("added:id");
          }, 5);
        });
      }
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    // 快速两次提交（输入同值模拟连点）；防重复提交挡掉第二次
    const addInput = w.find("input#source-add-claude");
    await addInput.setValue("D:/a");
    const addBtn = w
      .findAll("button")
      .find((b) => b.attributes("aria-label") === "Claude Code 添加目录");
    await addBtn!.trigger("click");
    await addBtn!.trigger("click");
    await flushPromises();
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "source_path_add").length;
    expect(calls).toBeLessThanOrEqual(2);
    expect(maxConcurrent, "同键追加必须串行").toBe(1);
    w.unmount();
  });

  it("sync_pricing_is_single_flight_and_keeps_old_prices_on_refresh_failure", async () => {
    const [gate, release] = deferred<unknown>();
    let syncCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "sync_pricing_openrouter") {
        syncCalls += 1;
        return gate; // 同步在途
      }
      // 同步后列表刷新失败：保留旧价格并归类为读取失败
      if (cmd === "pricing_entries") return Promise.reject(new Error("list down"));
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, close_action: null, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const btn = w.findAll("button").find((b) => b.text().includes("同步在线价格"))!;
    await btn.trigger("click");
    expect(syncCalls).toBe(1);
    // 在途时重复激活（快速连点/程序化触发）→ 函数级 guard 拒绝第二次
    await btn.trigger("click");
    await btn.trigger("click");
    expect(syncCalls, "同步进行中不得并发第二次请求").toBe(1);
    release([{ source: "models.dev", count: 5 }]);
    await flushPromises();
    expect(w.text()).toContain("价格列表读取失败");
    w.unmount();
  });

  it("sync_refresh_failure_keeps_previous_price_entries：列表刷新失败保留旧价格", async () => {
    // RC04：同步后的列表读取失败必须是"读取失败"，已显示的价格数据不得被
    // 清空（否则用户看到价格消失，误以为同步把数据弄坏了）。
    const withEntries = {
      ...pricingView,
      modelsdev_count: 3,
      entries: [
        {
          prefix: "vendor/keep-me",
          name: "Keep",
          channel: "vendor",
          input: 1,
          output: 2,
          cache_write: 0.2,
          cache_read: 0.1,
          base_incomplete: false,
          source: "models.dev",
          basis: null,
          segments: [],
          schedules: [],
          has_tiered_pricing: false,
          openrouter: null,
        },
      ],
    };
    let synced = false;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "sync_pricing_openrouter") {
        synced = true;
        return Promise.resolve([{ source: "models.dev", count: 3 }]);
      }
      if (cmd === "pricing_entries") {
        return synced ? Promise.reject(new Error("list down")) : Promise.resolve(withEntries);
      }
      if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, close_action: null, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w);
    // U05：价格视图在首次进入「价格」分区后读取。
    await openSection(w, "pricing");
    expect(vm.pricing?.entries.length, "进入价格分区后已有 1 条价格").toBe(1);

    await w
      .findAll("button")
      .find((b) => b.text().includes("同步在线价格"))!
      .trigger("click");
    await flushPromises();
    expect(w.text()).toContain("价格列表读取失败");
    expect(w.text()).toContain("保留上次数据");
    expect(w.text()).toContain("list down");
    expect(vm.pricing?.entries.length, "刷新失败必须保留旧价格数据").toBe(1);
    // 旧数据仍在界面上（优先级行的 models.dev 条数来自保留的价格视图）
    expect(w.text()).toContain("models.dev（3 条，主源）");
    // 归类为读取失败而非"无价格"：错误条仍提供该区块的重试入口
    expect(retryButtonFor(w, "价格列表读取失败"), "价格区块必须有重试").toBeDefined();
    w.unmount();
  });
});

// ── AP04：异步失败结果与写操作生命周期 ──
// 复核缺陷：loadSettings 只在成功分支检查 mutationEpoch，晚到的**失败**读取
// 会把刚保存的值降级成"未知"；各写操作 await 之后普遍没有 disposed 检查，
// 卸载后仍会弹成功通知；关闭动作快速连选会并发两个写请求（响应端忽略旧
// 结果并不能阻止后端按到达顺序落盘旧值）。
describe("Settings 写入生命周期与晚到失败（AP04）", () => {
  type Vm = SettingsVm & {
    agentEnabled: Record<string, boolean>;
    setAgentEnabled: (agent: "claude", v: boolean) => void;
    setAgentAuto: (agent: "claude", v: boolean) => void;
    syncPricing: () => Promise<void>;
    rebuild: () => Promise<void>;
  };

  function ctl<T>(): {
    promise: Promise<T>;
    resolve: (v: T) => void;
    reject: (e: unknown) => void;
  } {
    let resolve!: (v: T) => void;
    let reject!: (e: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    return { promise, resolve, reject };
  }

  function cmds(cmd: string): unknown {
    if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
    if (cmd === "pricing_entries") return Promise.resolve(pricingView);
    if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
    if (cmd === "autostart_status") return Promise.resolve(false);
    return Promise.resolve(null);
  }

  it("late_rejected_settings_read_does_not_reset_saved_value：晚到失败读取不降级已保存开关", async () => {
    const secondRead = ctl<unknown>();
    let settingsCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        settingsCalls += 1;
        if (settingsCalls === 1)
          return Promise.resolve({
            price_auto_sync: true,
            close_action: "ask",
            sources: {},
          });
        return secondRead.promise; // 旧刷新挂起
      }
      if (cmd === "settings_set_close_action") return Promise.resolve("quit");
      if (cmd === "settings_set_price_auto_sync") return Promise.resolve(false);
      if (cmd === "source_config_set") return Promise.resolve(null);
      return cmds(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w) as Vm;
    expect(vm.closeAction).toBe("ask");

    // 第二次读取挂起
    await w.setProps({ refreshKey: 1 });
    await flushPromises();
    expect(settingsCalls).toBe(2);

    // 挂起期间用户保存来源总开关、关闭动作与自动同步，均成功
    vm.setAgentEnabled("claude", false);
    await flushPromises();
    await vm.setCloseAction("quit");
    await vm.setAutoSync(false);
    expect(vm.closeAction).toBe("quit");
    expect(vm.autoSync).toBe(false);
    expect(vm.agentEnabled.claude).toBe(false);

    // 旧读取以失败结束：已保存状态都不得被降级或回退
    secondRead.reject(new Error("旧读取失败"));
    await flushPromises();
    expect(vm.closeAction, "晚到的失败读取不得覆盖已保存的关闭动作").toBe("quit");
    expect(vm.closeActionKnown, "已确认的状态不得被旧失败改回未知").toBe(true);
    expect(vm.autoSync, "晚到的失败读取不得把已保存的自动同步重置为未知").toBe(false);
    expect(vm.agentEnabled.claude, "晚到的失败读取不得回退来源总开关").toBe(false);
    expect(w.text(), "不得把旧读取的失败当成当前状态").not.toContain("旧读取失败");
    w.unmount();
  });

  it("settings_mutations_do_not_notify_after_unmount：卸载后写响应既不通知也不写状态", async () => {
    const gates: Record<string, { resolve: (v: unknown) => void; reject: (e: unknown) => void }> =
      {};
    const gate = (key: string) => {
      const c = ctl<unknown>();
      gates[key] = { resolve: c.resolve, reject: c.reject };
      return c.promise;
    };
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_config_set") return gate("source");
      if (cmd === "settings_set_price_auto_sync") return gate("autoSync");
      if (cmd === "settings_set_close_action") return gate("close");
      if (cmd === "sync_pricing_openrouter") return gate("sync");
      if (cmd === "refresh_cache") return gate("rebuild");
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, close_action: "ask", sources: {} });
      return cmds(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w) as Vm;
    const running = [
      vm.setAgentEnabled("claude", false),
      vm.setAutoSync(false),
      vm.setCloseAction("quit"),
      vm.syncPricing(),
      vm.rebuild(),
    ];
    await flushPromises();
    expect(Object.keys(gates).sort()).toEqual(["autoSync", "close", "rebuild", "source", "sync"]);

    msgSpy.success.mockClear();
    msgSpy.error.mockClear();
    msgSpy.info.mockClear();
    w.unmount();

    // 成功与失败两条路径都在卸载后落地
    gates.source.resolve({ ok: true });
    gates.autoSync.reject(new Error("auto sync down"));
    gates.close.resolve("quit");
    gates.sync.reject(new Error("sync down"));
    gates.rebuild.resolve({ path: "p", files: 1, events: 1 });
    await Promise.allSettled(running);
    await flushPromises();

    expect(msgSpy.success, "卸载后不得再发成功通知").not.toHaveBeenCalled();
    expect(msgSpy.error, "卸载后不得再发失败通知").not.toHaveBeenCalled();
    expect(msgSpy.info).not.toHaveBeenCalled();
  });

  it("close_action_rapid_changes_cannot_reorder：快速连选串行落地，UI 与最终持久化一致", async () => {
    const first = ctl<unknown>();
    const writes: (string | null)[] = [];
    invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "settings_set_close_action") {
        writes.push((args?.action ?? null) as string | null);
        return writes.length === 1 ? first.promise : Promise.resolve(args?.action ?? null);
      }
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, close_action: "ask", sources: {} });
      return cmds(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w) as Vm;

    const p1 = vm.setCloseAction("minimize");
    await flushPromises();
    expect(writes).toEqual(["minimize"]);
    // 第一次写入仍在途时的第二次选择：记为待写意图，不并发第二条请求
    const p2 = vm.setCloseAction("quit");
    await flushPromises();
    expect(writes, "在途写入期间不得并发第二条请求（否则后端可能乱序落盘）").toEqual(["minimize"]);
    expect(vm.closeAction, "UI 立即反映最后选择").toBe("quit");

    first.resolve("minimize");
    await Promise.allSettled([p1, p2]);
    await flushPromises();
    expect(writes, "当前写完成后立即补写最后选择").toEqual(["minimize", "quit"]);
    expect(vm.closeAction, "UI 与最后实际落盘的值一致").toBe("quit");
    expect(vm.closeActionKnown).toBe(true);
    w.unmount();
  });

  it("close_action_write_failure_rolls_back_to_persisted_value：写入失败回到已落盘值", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_set_close_action") return Promise.reject(new Error("disk full"));
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, close_action: "ask", sources: {} });
      return cmds(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w) as Vm;
    await vm.setCloseAction("minimize");
    await flushPromises();
    expect(msgSpy.error).toHaveBeenCalled();
    expect(vm.closeAction, "失败不得让界面停留在一个并未写入的选择").toBe("ask");
    expect(vm.closeActionKnown).toBe(true);
    w.unmount();
  });
});

// ── U05：设置页分区导航与用户可见文案 ─────────────────────────────
// 契约：左侧分区导航一次只显示一个分区（v-show 保留未保存草稿）；窄窗口
// 改为顶部分段控件；价格表在首次进入「价格」分区才挂载；页面上不得残留
// 内部审计编号与 markdown 星号。
describe("Settings 分区导航与文案（U05）", () => {
  /** 触发一次 CCS 预览：该段文案在修复前含审计编号 I-P3-2。 */
  const ccsPreview = {
    plan_id: "plan-u05",
    logical_source: "ccs",
    source_path: "C:/Users/x/.cc-switch/cc-switch.db",
    source_schema: "ccs-user-version-20",
    source_day_timezone: "Asia/Shanghai",
    history_generation: 3,
    generated_at: "2026-10-13T00:00:00Z",
    expires_in_seconds: 600,
    requests_total: 10,
    requests_importable: 8,
    requests_skipped_other_app: 1,
    requests_skipped_duplicate_of_proxy: 1,
    requests_rejected: 0,
    rejected_reasons: [],
    would_insert: 6,
    would_update: 0,
    would_unchanged: 2,
    would_conflict: 0,
    would_stale: 1,
    would_overlap: 0,
    overlap_unresolved: false,
    overlap_examples: [],
    net_new_tokens: { input: 100, output: 10, cache_write: 0, cache_read: 0 },
    rollups_total: 2,
    rollups_new: 2,
    rollups_unchanged: 0,
    rollups_conflicting: 0,
    rollups_rejected: 0,
    unsupported_apps: [],
    records_without_project: 0,
  };

  function u05Base(cmd: string): Promise<unknown> {
    if (cmd === "source_paths_list") return Promise.resolve(sourceRowsFixture);
    if (cmd === "pricing_entries") return Promise.resolve(pricingView);
    if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
    if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
    if (cmd === "autostart_status") return Promise.resolve(false);
    return Promise.resolve(null);
  }

  it("section_text_has_no_audit_id_or_markdown_stars：页面文本不含审计编号与字面星号", async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "ccs_import_preview" ? Promise.resolve(ccsPreview) : u05Base(cmd),
    );
    const w = mount(Settings);
    await flushPromises();
    // 进入「用量数据」分区并展开预览（该段在修复前含 I-P3-2）
    await openSection(w, "usage");
    await w
      .findAll("button")
      .find((b) => b.text().includes("从 CCS 导入用量"))!
      .trigger("click");
    await flushPromises();
    expect(w.text(), "预览段应已渲染").toContain("过期跳过");
    expect(w.text()).not.toContain("I-P3-2");
    expect(w.text(), "页面不得残留 markdown 星号").not.toContain("**");
    expect(settingsSource, "源文本不得残留内部审计编号").not.toContain("I-P3-2");
    // `**这个时区**` 的星号改为 .ts-strong 强调（不是把星号显示出来）
    expect(w.findAll(".ts-strong").map((el) => el.text())).toContain("这个时区");
    w.unmount();
  });

  it("switching_sections_keeps_unsaved_add_draft_without_saving", async () => {
    const w = mount(Settings);
    await flushPromises();
    await openSection(w, "sources");
    const addInput = () =>
      w.findAll("input").find((i) => i.attributes("aria-label") === "Claude Code 追加日志目录")!;
    await addInput().setValue("D:/draft-kept");
    await openSection(w, "pricing");
    // v-show 保留挂载：切走的分区节点仍在 DOM 中，组件状态（草稿）不销毁
    expect(w.find("[data-section='sources']").exists()).toBe(true);
    await openSection(w, "sources");
    expect((addInput().element as HTMLInputElement).value, "切分区不得清空未保存草稿").toBe(
      "D:/draft-kept",
    );
    expect(
      invokeMock.mock.calls.some((c) => c[0] === "source_path_add"),
      "切分区不得触发添加",
    ).toBe(false);
    w.unmount();
  });
});

// ── U06：数据源分区的自动读取间隔（唯一来源 settings.toml + 串行保存） ──
describe("Settings 自动读取间隔（U06）", () => {
  /** 挂载设置页并切到「数据源」分区（一次只显示一个分区）。 */
  async function mountSources(): Promise<VueWrapper> {
    const w = mount(Settings);
    await flushPromises();
    await openSection(w, "sources");
    return w;
  }

  function settingsBase(
    overrides: Record<string, (args?: Record<string, unknown>) => Promise<unknown>>,
  ): void {
    const base = invokeMock.getMockImplementation()!;
    invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
      overrides[cmd]
        ? overrides[cmd](args)
        : (base as (c: string, a?: Record<string, unknown>) => unknown)(cmd, args),
    );
  }

  function intervalRow(w: VueWrapper) {
    return w.findAll(".setting-row").find((r) => r.text().includes("自动读取间隔"));
  }

  it("interval_select_renders_in_sources_section：下拉在「数据源」分区且只用白名单六档", async () => {
    const w = await mountSources();
    const row = intervalRow(w);
    expect(row, "「数据源」分区必须有自动读取间隔设置项").toBeDefined();
    const select = row!.findComponent({ name: "Select" });
    expect(select.exists()).toBe(true);
    expect(select.props("options")).toEqual(COLLECT_INTERVAL_OPTIONS);
    w.unmount();
  });

  it("initial_value_comes_from_settings_get：初始值取自 collect_interval_secs（缺省 300）", async () => {
    settingsBase({ settings_get: () => Promise.resolve({ collect_interval_secs: 60 }) });
    const w = await mountSources();
    expect(useCollectInterval().intervalSecs.value).toBe(60);
    expect(intervalRow(w)!.findComponent({ name: "Select" }).props("value")).toBe(60);
    w.unmount();

    resetCollectIntervalForTests();
    settingsBase({ settings_get: () => Promise.resolve({ price_auto_sync: true, sources: {} }) });
    const w2 = await mountSources();
    expect(useCollectInterval().intervalSecs.value, "旧设置文件缺该字段 → 缺省 300").toBe(300);
    w2.unmount();
  });

  it("selecting_saves_through_serial_queue：选择值走 settings_set_collect_interval 保存", async () => {
    const written: unknown[] = [];
    settingsBase({
      settings_set_collect_interval: (args) => {
        written.push(args);
        return Promise.resolve(args?.secs);
      },
    });
    const w = await mountSources();
    intervalRow(w)!.findComponent({ name: "Select" }).vm.$emit("update:value", 5);
    await flushPromises();

    expect(written, "只发一次保存且参数是白名单值").toEqual([{ secs: 5 }]);
    expect(useCollectInterval().intervalSecs.value).toBe(5);
    expect(useCollectInterval().error.value).toBeNull();
    w.unmount();
  });

  it("save_failure_rolls_back_and_shows_error_only_in_that_row：失败回滚到已落盘值并只在该行提示", async () => {
    settingsBase({
      settings_set_collect_interval: () => Promise.reject(new Error("写入设置失败：磁盘不可写")),
    });
    const w = await mountSources();
    intervalRow(w)!.findComponent({ name: "Select" }).vm.$emit("update:value", 30);
    await flushPromises();

    expect(useCollectInterval().intervalSecs.value, "回滚到已落盘值").toBe(300);
    const row = intervalRow(w)!;
    expect(row.text()).toContain("保存失败");
    expect(row.text()).toContain("磁盘不可写");
    // 局部错误：其他分区不出现该提示
    expect(w.find("[data-section='general']").text()).not.toContain("磁盘不可写");
    w.unmount();
  });
});
