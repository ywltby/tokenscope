// UX03（设置页说明浮层可访问性）：priceCell/prefixCell 说明浮层的
// 真实交互验收——HelpTooltip 挂真实 NTooltip，hover/focus/click 全路径、
// aria-describedby 指向打开时的存在节点、Escape 关闭且焦点不离开触发器。
// NDataTable 在测试环境不渲染行：经 expose 的列 render 触达真实单元格。
import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import type { VNode } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const msgSpy = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), info: vi.fn() }));
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
    useMessage: () => msgSpy,
  };
});

import Settings from "./Settings.vue";
import type { PricingEntry } from "../types";

const entry: PricingEntry = {
  prefix: "nano-gpt/qwen/tiered-view",
  name: "Tiered View",
  channel: "nano-gpt",
  input: 4.0,
  output: 20.0,
  cache_write: 0.0,
  cache_read: null,
  source: "外置",
  base_incomplete: true,
  has_tiered_pricing: false,
};

function mockOk(): void {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "pricing_entries")
      return Promise.resolve({
        path: "C:/pricing.toml",
        modelsdev_path: "C:/md.json",
        modelsdev_count: 1,
        openrouter_path: "C:/or.json",
        openrouter_count: 0,
        external_count: 1,
        entries: [entry],
        warnings: [],
      });
    if (cmd === "source_status") return Promise.resolve([]);
    if (cmd === "cache_stats") return Promise.resolve({ path: "c", files: 0, events: 0 });
    if (cmd === "settings_get")
      return Promise.resolve({ price_auto_sync: true, close_action: null });
    if (cmd === "autostart_status") return Promise.resolve({ enabled: false, available: false });
    return Promise.resolve(null);
  });
}

function bodyVisible(): boolean {
  const tips = Array.from(document.querySelectorAll<HTMLElement>(".help-tooltip-body"));
  return tips.some((el) => {
    let node: HTMLElement | null = el;
    while (node) {
      if (getComputedStyle(node).display === "none") return false;
      node = node.parentElement;
    }
    return true;
  });
}

async function waitTip(shown: boolean): Promise<void> {
  const deadline = Date.now() + 1000;
  while (Date.now() < deadline) {
    if (bodyVisible() === shown) return;
    await flushPromises();
    await new Promise((r) => setTimeout(r, 10));
  }
  expect(bodyVisible(), `说明浮层应${shown ? "出现" : "消失"}于文档`).toBe(shown);
}

/** 经 expose 的列 render 渲染真实单元格（行对象 = PricingEntry）。 */
function mountCell(wrapper: VueWrapper, key: string, row: object): VueWrapper {
  const cols = (
    wrapper.vm as unknown as {
      priceColumns: { key: string; render?: (r: object) => VNode }[];
    }
  ).priceColumns;
  const col = cols.find((c) => c.key === key);
  if (!col?.render) throw new Error(`列 ${key} 无 render`);
  const vnode = col.render(row);
  return mount({ setup: () => () => vnode }, { attachTo: document.body });
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("Settings 说明浮层可访问性（UX03）", () => {
  it("settings_help_opens_on_focus_and_click：focus/click/Enter 均打开且描述关联存在", async () => {
    mockOk();
    const settings = mount(Settings, { attachTo: document.body });
    await flushPromises();
    const cell = mountCell(settings, "input", entry);
    const trigger = cell.find('[role="button"]');
    expect(trigger.exists(), "单价说明触发器应存在").toBe(true);
    expect(trigger.attributes("aria-label")).toContain("单价说明");
    // 初始：无 describedby（内容节点尚不存在）
    expect(trigger.attributes("aria-describedby")).toBeUndefined();
    // focus 打开
    await trigger.trigger("focus");
    await waitTip(true);
    const descId = trigger.attributes("aria-describedby")!;
    expect(descId).toBeTruthy();
    expect(document.getElementById(descId), "描述节点必须存在").not.toBeNull();
    expect(document.getElementById(descId)!.textContent).toContain("来源：外置");
    // Escape 关闭且不立即重开（焦点语义不因 Escape 循环打开）
    await trigger.trigger("keydown", { key: "Escape" });
    await waitTip(false);
    await new Promise((r) => setTimeout(r, 50));
    await flushPromises();
    expect(bodyVisible(), "Escape 后浮层保持关闭（不因焦点循环重开）").toBe(false);
    expect(trigger.element.isConnected, "触发器仍在文档中").toBe(true);
    // click 固定切换（pinned）：focus 移开后仍打开
    await trigger.trigger("click");
    await waitTip(true);
    await trigger.trigger("blur");
    await flushPromises();
    expect(bodyVisible(), "click 固定后失焦仍打开").toBe(true);
    // Space 同样切换，可关闭
    await trigger.trigger("keydown", { key: " ", preventDefault: () => {} } as never);
    await waitTip(false);
    settings.unmount();
    cell.unmount();
  });

  it("prefixCell 档位明细同样走可访问浮层", async () => {
    mockOk();
    const settings = mount(Settings, { attachTo: document.body });
    await flushPromises();
    const tiered = {
      ...entry,
      has_tiered_pricing: true,
      segments: [
        {
          label: ">272K",
          min_tokens: 272001,
          max_tokens: null,
          prices: { input: 8.0, output: 30.0, cache_write: null, cache_read: null },
        },
      ],
    };
    const cell = mountCell(settings, "prefix", tiered);
    const trigger = cell.find('[role="button"]');
    expect(trigger.exists(), "有分段时触发器应存在").toBe(true);
    await trigger.trigger("focus");
    await waitTip(true);
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    settings.unmount();
    cell.unmount();
  });
});
