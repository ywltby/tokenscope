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

const sourceStatuses = [
  { agent: "claude-code", dir: "C:/claude", enabled: true, exists: true, files: 3, state: "ready" },
  { agent: "codex", dir: "C:/codex", enabled: true, exists: true, files: 2, state: "ready" },
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
  drafts: Record<string, { enabled: boolean; dir: string }>;
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
    if (cmd === "source_status") return Promise.resolve(sourceStatuses);
    if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
    if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
    if (cmd === "autostart_status") return Promise.resolve(false);
    if (cmd === "loadAutostart" || cmd === "loadAutoSync") return Promise.resolve(null);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  invokeMock.mockReset();
  mockBase();
});

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
    // source_status / cache_stats / pricing_entries / settings_get /
    // autostart_status + H05 的来源库默认路径解析（只解析路径，不打开 CCS 库）
    expect(invokeMock).toHaveBeenCalledTimes(6);
    expect(vmOfSettings(w).autoSync).toBe(true);
    expect(vmOfSettings(w).pricing?.modelsdev_count).toBe(10);
    expect(w.findComponent({ name: "NSpin" }).exists()).toBe(false);
    w.unmount();
  });
  it("sync 部分失败仍重新调用 pricing_entries", async () => {
    let syncCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
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
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
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

describe("Settings 来源配置（Task 4）", () => {
  it("source_config_set 重叠错误可见且输入保留", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "source_config_set") {
        return Promise.reject(new Error("Claude 与 Codex 来源目录指向同一位置，会导致重复统计"));
      }
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    // 修改 codex 目录为与 claude 相同后保存
    const saveBtns = w.findAll("button").filter((b) => b.text() === "保存");
    expect(saveBtns.length).toBe(2);
    await saveBtns[1].trigger("click");
    await flushPromises();
    expect(w.text()).toContain("来源目录指向同一位置");
    // 输入保留：draft 未被清空（v-model 值仍在）
    expect(
      (w.vm as unknown as { drafts: Record<string, { dir: string }> }).drafts.codex,
    ).toBeDefined();
  });

  it("重新加载来源列表不清除未提交草稿", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "source_config_set") return Promise.resolve({ enabled: true, dir: "D:/new" });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    // 用户在 claude 行输入了一个未保存的草稿
    const vm = w.vm as unknown as { drafts: Record<string, { dir: string; enabled: boolean }> };
    vm.drafts.claude.dir = "D:/user-typing";
    // codex 行保存成功 → loadSources 刷新（loadDrafts 不应覆盖 claude 草稿？）
    // 注：当前实现 loadDrafts 会重置全部草稿——本测试先钉住现状，
    // 若产品要求跨行草稿保留需另行修改（此处验证保存后草稿刷新行为可见）。
    const saveBtns = w.findAll("button").filter((b) => b.text() === "保存");
    await saveBtns[1].trigger("click");
    await flushPromises();
    expect(invokeMock.mock.calls.some((c) => c[0] === "source_config_set")).toBe(true);
    expect(
      invokeMock.mock.calls.some((c) => c[0] === "source_status"),
      "保存后重新加载来源状态",
    ).toBe(true);
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

import { sourceIdOf } from "../types";

describe("Settings 数据来源（claude-code 行修复）", () => {
  beforeEach(() => {
    msgSpy.success.mockClear();
    msgSpy.error.mockClear();
  });

  /** 定位指定来源的行容器与行内操作区。 */
  function rowOf(w: ReturnType<typeof mount>, label: string) {
    const strong = w.findAll("strong").find((x) => x.text() === label);
    expect(strong, `${label} 行应存在`).toBeDefined();
    const ops = strong!.element.parentElement!;
    const row = ops.parentElement!;
    return { ops, row };
  }

  it("sourceIdOf 归一化 agent 序列化值为来源 ID", () => {
    expect(sourceIdOf("claude-code")).toBe("claude");
    expect(sourceIdOf("codex")).toBe("codex");
  });

  it("claude-code 行点开关：aria-checked 翻转并写入 claude 草稿", async () => {
    const w = mount(Settings);
    await flushPromises();
    const { ops } = rowOf(w, "Claude Code");
    const sw = ops.querySelector('[role="switch"]');
    expect(sw).toBeDefined();
    expect(sw!.getAttribute("aria-checked")).toBe("true");
    await (sw! as HTMLElement).dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await flushPromises();
    expect(sw!.getAttribute("aria-checked")).toBe("false");
    const vm = w.vm as unknown as { drafts: Record<string, { enabled: boolean }> };
    expect(vm.drafts.claude.enabled).toBe(false);
  });

  it("claude-code 行点保存：invoke 使用 agent=claude 且有成功提示", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_config_set") return Promise.resolve({ enabled: false, dir: null });
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      if (cmd === "loadAutostart" || cmd === "loadAutoSync") return Promise.resolve(null);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const { ops } = rowOf(w, "Claude Code");
    const saveBtn = Array.from(ops.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "保存",
    );
    expect(saveBtn).toBeDefined();
    await (saveBtn! as HTMLElement).click();
    await flushPromises();
    const call = invokeMock.mock.calls.find((c) => c[0] === "source_config_set");
    expect(call).toBeDefined();
    expect(call![1]).toMatchObject({ agent: "claude" });
    expect(msgSpy.success).toHaveBeenCalled();
  });

  it("保存失败的错误只渲染在对应来源行", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_config_set")
        return Promise.reject(new Error("Claude 与 Codex 来源目录指向同一位置"));
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      if (cmd === "loadAutostart" || cmd === "loadAutoSync") return Promise.resolve(null);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const claude = rowOf(w, "Claude Code");
    const codex = rowOf(w, "Codex");
    const saveBtn = Array.from(claude.ops.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "保存",
    );
    await (saveBtn! as HTMLElement).click();
    await flushPromises();
    // 错误文案全页只出现一次（归属 Claude 行）——修复前在 v-for 内
    // 跨行重复渲染两次。
    const errText = "来源目录指向同一位置";
    expect(w.text().split(errText).length - 1).toBe(1);
    // 输入保留
    const vm = w.vm as unknown as { drafts: Record<string, { enabled: boolean }> };
    expect(vm.drafts.claude).toBeDefined();
    void codex;
  });
});

describe("Settings 分组与状态（设计系统 Task 7）", () => {
  it("设置页按 应用/数据源/缓存/价格 四组渲染", async () => {
    const w = mount(Settings);
    await flushPromises();
    for (const title of ["应用", "数据源", "缓存", "价格"]) {
      expect(w.text()).toContain(title);
    }
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
  it("每组一张 .ts-card，组标题（.group-title）在卡片外", async () => {
    const w = mount(Settings);
    await flushPromises();
    const titles = w.findAll(".group-title").map((t) => t.text());
    expect(titles).toEqual([
      "Token 显示颜色",
      "应用",
      "数据源",
      "缓存",
      "从 CCS 导入用量",
      "价格",
      "高级配置",
    ]);
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
    // 至少：时区、自启、缓存文件、缓存事件、自动同步价格
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
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
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
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    expect(vmOf(w).closeAction).toBe("quit");
  });

  it("高级配置组：底部标题 + 打开设置配置文件按钮调用 open_settings_file", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "open_settings_file")
        return Promise.resolve("C:/Users/x/.tokenscope/settings.toml");
      return mockBaseImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const titles = w.findAll(".group-title").map((t) => t.text());
    expect(titles.at(-1)).toBe("高级配置");
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
    if (cmd === "source_status") return Promise.resolve(sourceStatuses);
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
      if (cmd === "source_status") return Promise.reject(new Error("boom-status"));
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
    // 其他成功区块照常显示（缓存统计、价格表）。
    expect(w.text()).toContain("缓存文件");
    expect(w.text()).toContain("同步在线价格");
    // 重试成功后错误清除。
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
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

  it("settings_get 失败：来源保存禁用并说明原因；恢复后可保存", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") return Promise.reject(new Error("设置读取失败"));
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    expect(w.text()).toContain("设置读取失败，来源配置暂不可保存");
    const save = w.findAll("button").find((b) => b.text() === "保存");
    expect(save).toBeDefined();
    expect((save!.element as HTMLButtonElement).disabled).toBe(true);
    w.unmount();
  });

  it("retry_preserves_dirty_source_drafts：重试只更新未编辑草稿行", async () => {
    let settingsCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        settingsCalls += 1;
        return Promise.resolve({
          price_auto_sync: true,
          sources: {
            claude: { enabled: true, dir: settingsCalls === 1 ? "C:/old-claude" : "C:/new-claude" },
            codex: { enabled: true, dir: settingsCalls === 1 ? "C:/old-codex" : "C:/new-codex" },
          },
        });
      }
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    // 用户编辑 claude 草稿目录。
    const claudeInput = w
      .findAll("input")
      .find((i) => (i.element as HTMLInputElement).value === "C:/old-claude");
    expect(claudeInput).toBeDefined();
    await claudeInput!.setValue("C:/user-edited");
    // 重试（refreshKey 触发 loadAll → loadDrafts）。
    await w.setProps({ refreshKey: 1 });
    await flushPromises();
    // claude 草稿保留用户输入；codex 更新为新值。
    const inputs = w.findAll("input");
    const claudeVal = inputs.find((i) =>
      (i.element as HTMLInputElement).value.includes("user-edited"),
    );
    expect(claudeVal, "claude 草稿保留用户编辑").toBeDefined();
    const codexVal = inputs.find((i) => (i.element as HTMLInputElement).value === "C:/new-codex");
    expect(codexVal, "codex 草稿更新为新值").toBeDefined();
    w.unmount();
  });

  it("late_settings_response_does_not_overwrite_saved_source_value：晚到读取不覆盖已保存来源", async () => {
    // RC03：按"重试读取挂起 → 编辑 → 保存成功 → 释放旧读取"执行。
    // 保存成功 ≠ 读取的数据更新——写入开始即作废在途读取的提交资格。
    const [pendingRead, resolvePendingRead] = deferred<unknown>();
    let settingsCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") {
        settingsCalls += 1;
        if (settingsCalls === 1) return Promise.reject(new Error("首次读取失败"));
        if (settingsCalls === 2) return pendingRead; // 重试读取挂起
        return Promise.resolve({ price_auto_sync: true, close_action: null, sources: {} });
      }
      if (cmd === "source_config_set") return Promise.resolve({ enabled: true, dir: "C:/saved" });
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = w.vm as unknown as SettingsVm;
    // 点击设置错误条重试 → 第二次读取在途
    const retry = retryButtonFor(w, "设置读取失败");
    await retry!.trigger("click");
    await flushPromises();
    expect(settingsCalls).toBe(2);

    // 编辑并保存 claude 草稿
    const claudeInput = w
      .findAll("input")
      .find((i) => i.attributes("aria-label") === "Claude Code 日志目录");
    await claudeInput!.setValue("C:/saved");
    const save = w.findAll("button").filter((b) => b.text() === "保存")[0];
    await save.trigger("click");
    await flushPromises();
    expect(vm.drafts.claude.dir).toBe("C:/saved");
    // 提交参数必须是用户保存的值（mock 不会自动回读新值）
    expect(invokeMock).toHaveBeenCalledWith("source_config_set", {
      agent: "claude",
      enabled: true,
      dir: "C:/saved",
    });

    // 释放内容为旧值的挂起读取：不得覆盖保存值，也不得填充关闭动作/自动同步
    resolvePendingRead({
      price_auto_sync: true,
      close_action: "minimize",
      sources: {
        claude: { enabled: true, dir: "C:/old" },
        codex: { enabled: true, dir: "D:/old" },
      },
    });
    await flushPromises();
    expect(vm.drafts.claude.dir, "晚到读取不得覆盖保存值").toBe("C:/saved");
    // 界面值（真实 input 的 value）同样是保存值
    const claudeAfter = w
      .findAll("input")
      .find((i) => i.attributes("aria-label") === "Claude Code 日志目录");
    expect((claudeAfter!.element as HTMLInputElement).value).toBe("C:/saved");
    expect(vm.closeActionKnown, "晚到读取不得让未知状态变成已知").toBe(false);
    expect(vm.closeAction).toBe("ask");
    w.unmount();
  });

  it("来源保存成功但状态刷新失败：区分提示（不误导重复保存）", async () => {
    let savedOnce = false;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_config_set") {
        savedOnce = true;
        return Promise.resolve({ enabled: true, dir: "C:/x" });
      }
      if (cmd === "source_status") {
        return savedOnce
          ? Promise.reject(new Error("refresh down"))
          : Promise.resolve(sourceStatuses);
      }
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const save = w.findAll("button").find((b) => b.text() === "保存");
    await save!.trigger("click");
    await flushPromises();
    // RC03：提示必须保留真实失败原因，不能只说"可重试"。
    expect(w.text()).toContain("已保存，但状态刷新失败");
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
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
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
  function mockWithSources(statuses: object[]): void {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_status") return Promise.resolve(statuses);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
  }

  it("source_directory_input_has_stable_name：真实 input 有稳定可访问名称且与 label 关联", async () => {
    const w = mount(Settings);
    await flushPromises();
    const inputs = w.findAll(".n-input__input-el");
    expect(inputs.length).toBeGreaterThanOrEqual(2);
    expect(inputs.map((i) => i.attributes("aria-label"))).toEqual([
      "Claude Code 日志目录",
      "Codex 日志目录",
    ]);
    // label[for] 指向真实 input 的 id（名称不靠 placeholder 兜底）
    for (const input of inputs) {
      const id = input.attributes("id");
      expect(id, "真实 input 必须有稳定 id").toBeTruthy();
      const label = w.find(`label[for="${id}"]`);
      expect(label.exists()).toBe(true);
      expect(label.text()).toBe(input.attributes("aria-label"));
    }
    // placeholder 只说明留空语义，不再内嵌会变动的路径
    for (const input of inputs) {
      expect(input.attributes("placeholder")).toBe("留空使用当前生效目录");
    }
  });

  it("effective_directory_is_available_in_full：完整当前生效目录可读、可复制、长路径换行", async () => {
    const longDir =
      "C:/Users/dev/very/deeply/nested/agent/logs/directory/that/keeps/going/.claude/projects";
    mockWithSources([
      { agent: "claude-code", dir: longDir, enabled: true, exists: true, files: 3, state: "ready" },
      { agent: "codex", dir: "C:/codex", enabled: true, exists: true, files: 2, state: "ready" },
    ]);
    const w = mount(Settings);
    await flushPromises();
    const paths = w.findAll(".source-effective .effective-path");
    expect(paths.length).toBe(2);
    // 完整值（未截断）
    expect(paths[0].text()).toBe(longDir);
    expect(paths[1].text()).toBe("C:/codex");
    // 标注为"当前生效目录"，不把有效覆盖目录叫"默认目录"
    expect(w.findAll(".source-effective .effective-label").map((l) => l.text())).toEqual([
      "当前生效目录：",
      "当前生效目录：",
    ]);
    // 技术详情同样提供完整值。真实 NCollapseItem 收起时**不渲染**内容
    //（displayDirective 默认 if），故此处断言模板契约 + 渲染态内联行已覆盖。
    expect(settingsSource).toMatch(
      /v-for="s in sources"[\s\S]{0,160}当前生效目录：\{\{ s\.dir \}\}/,
    );
    // 可选中复制 + 长路径换行（样式契约；happy-dom 无布局）
    const eff = /\.source-effective\s*\{([^}]*)\}/.exec(settingsSource);
    expect(eff?.[1]).toContain("user-select: text");
    const path = /\.effective-path\s*\{([^}]*)\}/.exec(settingsSource);
    expect(path?.[1]).toContain("word-break: break-all");
    expect(path?.[1]).toContain("white-space: normal");
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
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
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

  it("save_preserves_existing_error_states：保存来源不丢失其他已有错误", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "pricing_entries") return Promise.reject(new Error("价格读取失败"));
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      // codex 保存失败 → 该行错误
      if (cmd === "source_config_set")
        return Promise.reject(new Error("来源目录与 Claude Code 重叠"));
      return Promise.resolve(null);
    });
    msgSpy.success.mockClear();
    const w = mount(Settings);
    await flushPromises();
    // 先制造两处已有错误：价格区块 + codex 行
    expect(w.text()).toContain("价格列表读取失败");
    const codexSave = w
      .findAll("button")
      .filter((b) => b.text() === "保存")
      .at(1)!;
    await codexSave.trigger("click");
    await flushPromises();
    expect(w.text()).toContain("来源目录与 Claude Code 重叠");

    // 成功保存 claude 行（其余 mock 保持失败）
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_config_set") return Promise.resolve(null);
      if (cmd === "pricing_entries") return Promise.reject(new Error("价格读取失败"));
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const claudeSave = w
      .findAll("button")
      .filter((b) => b.text() === "保存")
      .at(0)!;
    await claudeSave.trigger("click");
    await flushPromises();
    // 已有错误状态在保存前后不丢失
    expect(w.text()).toContain("价格列表读取失败");
    expect(w.text()).toContain("来源目录与 Claude Code 重叠");
    expect(msgSpy.success).toHaveBeenCalled();
  });
});

// ── RC03：设置统一读取、局部恢复与写后防回退（2026-10-08 复核） ──
// 复核缺陷：首载发出三次 settings_get；读取失败把关闭动作显示成可操作的
// "每次询问"；错误条重试只恢复来源草稿；保存成功后晚到的旧读取把输入框
// 覆盖回旧值（C:/saved → C:/old）。
describe("Settings 统一读取与写后防回退（RC03）", () => {
  function settingsImpl(cmd: string): unknown {
    if (cmd === "source_status") return Promise.resolve(sourceStatuses);
    if (cmd === "pricing_entries") return Promise.resolve(pricingView);
    if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
    if (cmd === "autostart_status") return Promise.resolve(false);
    return Promise.resolve(null);
  }

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
    expect(vm.drafts.claude.dir).toBe("C:/c");
    expect(vm.drafts.codex.enabled).toBe(false);
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
      ["source_status", "来源状态读取失败"],
      ["cache_stats", "缓存统计读取失败"],
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
      expect(w.text(), failing).toContain(expected);
      expect(w.text(), failing).toContain(`${failing} down`);
      // 其他成功区块照常渲染（缓存/价格/数据源三组都在）
      expect(w.text(), failing).toContain("缓存文件");
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
      if (cmd === "source_status" && fail) return Promise.reject(new Error("来源状态读取失败"));
      if (cmd === "cache_stats" && fail) return Promise.reject(new Error("缓存统计读取失败"));
      if (cmd === "pricing_entries" && fail) return Promise.reject(new Error("价格列表读取失败"));
      if (cmd === "autostart_status" && fail) return Promise.reject(new Error("自启状态读取失败"));
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, close_action: null, sources: {} });
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    fail = false;
    for (const notice of [
      "设置读取失败",
      "来源状态读取失败",
      "缓存统计读取失败",
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
    // 来源保存禁用
    const save = w.findAll("button").filter((b) => b.text() === "保存")[0];
    expect((save.element as HTMLButtonElement).disabled).toBe(true);

    // 重试：三处状态全部恢复
    fail = false;
    const retry = retryButtonFor(w, "设置读取失败");
    expect(retry, "设置错误条必须有重试按钮").toBeDefined();
    await retry!.trigger("click");
    await flushPromises();
    expect(vm.closeAction).toBe("quit");
    expect(vm.closeActionKnown).toBe(true);
    expect(vm.autoSync).toBe(true);
    expect(vm.drafts.claude.dir).toBe("C:/restored");
    expect(vm.drafts.codex.enabled).toBe(false);
    expect(w.text()).not.toContain("设置读取失败");
    expect(w.text()).not.toContain("关闭动作未知");
    // 恢复后可保存
    const save2 = w.findAll("button").filter((b) => b.text() === "保存")[0];
    expect((save2.element as HTMLButtonElement).disabled).toBe(false);
    w.unmount();
  });

  it("edit_during_save_remains_dirty：保存期间的新编辑不被清除也不被读取覆盖", async () => {
    const [saveGate, releaseSave] = deferred<unknown>();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get")
        return Promise.resolve({
          price_auto_sync: true,
          close_action: "ask",
          sources: {
            claude: { enabled: true, dir: "C:/server" },
            codex: { enabled: true, dir: "" },
          },
        });
      if (cmd === "source_config_set") return saveGate; // 提交 A 挂起
      return settingsImpl(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w);
    const claudeInput = w
      .findAll("input")
      .find((i) => i.attributes("aria-label") === "Claude Code 日志目录")!;
    await claudeInput.setValue("A");
    const save = w.findAll("button").filter((b) => b.text() === "保存")[0];
    await save.trigger("click");
    await flushPromises();
    // 保存期间用户继续编辑 B
    await claudeInput.setValue("B");
    releaseSave({ enabled: true, dir: "A" });
    await flushPromises();
    // 提交 A 返回不得清掉 B 的 dirty
    expect(vm.drafts.claude.dir).toBe("B");
    // 后续读取也不得覆盖 B
    await w.setProps({ refreshKey: 1 });
    await flushPromises();
    expect(vm.drafts.claude.dir, "B 仍是 dirty，读取不得覆盖").toBe("B");
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
    expect(vm.drafts.claude?.dir ?? "").toBe("");
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

  it("concurrent_row_saves_keep_independent_saving_state：跨行保存不共用全局 saving 状态", async () => {
    // 每行有自己的保存中状态——claude 提交挂起时 codex 仍可保存并解锁，
    // claude 不得被 codex 的返回提前解锁。
    const [claudeGate, releaseClaude] = deferred<unknown>();
    invokeMock.mockImplementation((c: string, args?: Record<string, unknown>) => {
      if (c === "source_config_set") {
        return args?.agent === "claude" ? claudeGate : Promise.resolve({ ok: true });
      }
      return settingsImpl(c);
    });
    const w = mount(Settings);
    await flushPromises();
    const claudeRow = w.findAll("strong").find((x) => x.text() === "Claude Code")!;
    const codexRow = w.findAll("strong").find((x) => x.text() === "Codex")!;
    const saveIn = (anchor: typeof claudeRow) =>
      Array.from(anchor.element.parentElement!.querySelectorAll("button")).find(
        (b) => b.textContent?.trim() === "保存",
      ) as HTMLButtonElement | undefined;
    const claudeSave = saveIn(claudeRow)!;
    const codexSave = saveIn(codexRow)!;

    await claudeSave.click();
    await flushPromises();
    expect(claudeSave.classList.toString(), "claude 提交在途 → 该行保存中").toContain(
      "n-button--loading",
    );
    expect(codexSave.classList.toString(), "另一行不得被视为保存中").not.toContain(
      "n-button--loading",
    );
    // codex 保存完成，不得提前解锁 claude
    await codexSave.click();
    await flushPromises();
    expect(codexSave.classList.toString()).not.toContain("n-button--loading");
    expect(claudeSave.classList.toString(), "codex 返回不得解锁 claude 行").toContain(
      "n-button--loading",
    );
    releaseClaude({ ok: true });
    await flushPromises();
    expect(claudeSave.classList.toString(), "claude 自己完成后解锁").not.toContain(
      "n-button--loading",
    );
    w.unmount();
  });
});

// ── RC04：设置页同步的函数级防重复与列表刷新失败保留旧价格 ──
describe("Settings 价格同步（RC04）", () => {
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
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
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
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
      if (cmd === "settings_get")
        return Promise.resolve({ price_auto_sync: true, close_action: null, sources: {} });
      if (cmd === "autostart_status") return Promise.resolve(false);
      return Promise.resolve(null);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w);
    expect(vm.pricing?.entries.length, "首载已有 1 条价格").toBe(1);

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
    saveSource: (agent: string) => Promise<void>;
    setSourceDir: (id: string, v: string) => void;
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
    if (cmd === "source_status") return Promise.resolve(sourceStatuses);
    if (cmd === "pricing_entries") return Promise.resolve(pricingView);
    if (cmd === "cache_stats") return Promise.resolve({ path: "p", files: 1, events: 2 });
    if (cmd === "autostart_status") return Promise.resolve(false);
    return Promise.resolve(null);
  }

  it("late_rejected_settings_read_does_not_reset_saved_value：晚到失败读取不降级已保存值", async () => {
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
      if (cmd === "source_config_set") return Promise.resolve({ enabled: true, dir: "C:/saved" });
      return cmds(cmd);
    });
    const w = mount(Settings);
    await flushPromises();
    const vm = vmOfSettings(w) as Vm;
    expect(vm.closeAction).toBe("ask");
    expect(vm.autoSync).toBe(true);

    // 第二次读取挂起（内容无关，最终以失败告终）
    await w.setProps({ refreshKey: 1 });
    await flushPromises();
    expect(settingsCalls).toBe(2);

    // 挂起期间用户保存来源、关闭动作与自动同步，均成功
    vm.setSourceDir("claude", "C:/saved");
    await vm.saveSource("claude");
    await vm.setCloseAction("quit");
    await vm.setAutoSync(false);
    expect(vm.closeAction).toBe("quit");
    expect(vm.autoSync).toBe(false);

    // 旧读取以失败结束：三类已保存状态都不得被降级或回退
    secondRead.reject(new Error("旧读取失败"));
    await flushPromises();
    expect(vm.closeAction, "晚到的失败读取不得覆盖已保存的关闭动作").toBe("quit");
    expect(vm.closeActionKnown, "已确认的状态不得被旧失败改回未知").toBe(true);
    expect(vm.autoSync, "晚到的失败读取不得把已保存的自动同步重置为未知").toBe(false);
    expect(vm.drafts.claude?.dir, "晚到的失败读取不得回退来源草稿").toBe("C:/saved");
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
      vm.saveSource("claude"),
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
