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
    // RC06：说明触发器是原生 button（浏览器内建 Enter/Space 激活）。
    const trigger = cell.find("button.help-trigger");
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
    // RC06：触发器是原生 button —— Enter/Space 由**浏览器合成一次 click**，
    // 组件不得再手写 keydown 处理（否则重复 toggle）。因此单独 keydown
    // 不改变状态，而合成 click（等价于 Enter/Space 的结果）才切换。
    expect(trigger.element.tagName, "触发器应为原生 button").toBe("BUTTON");
    expect(trigger.attributes("type")).toBe("button");
    await trigger.trigger("keydown", { key: " ", preventDefault: () => {} } as never);
    await flushPromises();
    expect(bodyVisible(), "单独 keydown 不得 toggle").toBe(true);
    await trigger.trigger("click");
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
    // RC06：说明触发器是原生 button（浏览器内建 Enter/Space 激活）。
    const trigger = cell.find("button.help-trigger");
    expect(trigger.exists(), "有分段时触发器应存在").toBe(true);
    await trigger.trigger("focus");
    await waitTip(true);
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    settings.unmount();
    cell.unmount();
  });
});

// ── RC06：单价列名称由显式 key 决定；说明浮层外部点击关闭 ──
// 复核缺陷：`columnLabelOf(pick)` 用函数对象比较，四列全部显示成
// 「缓存命中单价说明」；HelpTooltip 缺少外部点击关闭。
describe("Settings 单价列名称与外部点击（RC06）", () => {
  it("each_price_column_exposes_its_own_label：四列各自可访问名称正确", async () => {
    mockOk();
    const settings = mount(Settings, { attachTo: document.body });
    await flushPromises();
    const expectLabel: [string, string][] = [
      ["input", "输入"],
      ["output", "输出"],
      ["cache_write", "缓存写"],
      ["cache_read", "缓存命中"],
    ];
    for (const [key, label] of expectLabel) {
      const cell = mountCell(settings, key, entry);
      const trigger = cell.find("button.help-trigger");
      expect(trigger.exists(), `${key} 列触发器应存在`).toBe(true);
      expect(trigger.attributes("aria-label"), `${key} 列名称`).toBe(
        `${entry.prefix} ${label}单价说明`,
      );
      cell.unmount();
    }
    // 列头同样来自 tokenDisplay（不再散落「缓存读$」）
    const cols = (settings.vm as unknown as { priceColumns: { key: string; title: string }[] })
      .priceColumns;
    const titleOf = (k: string) => cols.find((c) => c.key === k)!.title;
    expect(titleOf("input")).toContain("输入");
    expect(titleOf("cache_read")).toContain("缓存命中");
    expect(titleOf("cache_read")).not.toContain("缓存读");
    settings.unmount();
  });

  it("help_tooltip_closes_on_outside_click：固定后点外部关闭且可再次激活", async () => {
    mockOk();
    const settings = mount(Settings, { attachTo: document.body });
    await flushPromises();
    const cell = mountCell(settings, "input", entry);
    const trigger = cell.find("button.help-trigger");
    // 固定（click）
    await trigger.trigger("click");
    await waitTip(true);
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    // 移开指针（hover 不参与固定态）
    await trigger.trigger("mouseleave");
    await flushPromises();
    expect(bodyVisible(), "固定后移开指针仍打开").toBe(true);
    // 点击外部 → 关闭、描述关联清除
    document.body.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await waitTip(false);
    expect(trigger.attributes("aria-describedby")).toBeUndefined();
    // 触发器仍可再次激活
    await trigger.trigger("click");
    await waitTip(true);
    expect(trigger.attributes("aria-describedby")).toBeTruthy();
    settings.unmount();
    cell.unmount();
  });
});

// ── RC07：单价量纲在列头与说明中显式（USD / 1M token） ──
describe("Settings 单价量纲（RC07）", () => {
  it("unit_dimension_is_explicit_in_headers_and_help", async () => {
    mockOk();
    const settings = mount(Settings, { attachTo: document.body });
    await flushPromises();
    const cols = (settings.vm as unknown as { priceColumns: { key: string; title: string }[] })
      .priceColumns;
    for (const k of ["input", "output", "cache_write", "cache_read"]) {
      const title = cols.find((c) => c.key === k)!.title;
      expect(title, `${k} 列头应标注量纲`).toContain("$/M");
    }
    // 表格附近的说明给出完整单位（每百万 token）
    expect(settings.text()).toContain("USD / 1M token");
    expect(settings.text()).toContain("每百万 token");
    settings.unmount();
  });
});

// ── RC06.4：同时挂载多行时，描述关联不得互相串用 ──
describe("Settings 多行浮层身份（RC06）", () => {
  function rowOf(prefix: string, orName: string): PricingEntry {
    return {
      ...entry,
      prefix,
      openrouter: {
        input: 1.0,
        output: 2.0,
        cache_write: null,
        cache_read: null,
        name: orName,
      },
    };
  }

  /** 同一应用实例内渲染两行（useId 的作用域是应用实例；分成两个 mount
   *  会让两份 id 各自从 0 起，测不到真实情况）。 */
  function renderCell(wrapper: VueWrapper, key: string, row: object): VNode {
    const cols = (
      wrapper.vm as unknown as {
        priceColumns: { key: string; render?: (r: object) => VNode }[];
      }
    ).priceColumns;
    const col = cols.find((c) => c.key === key);
    if (!col?.render) throw new Error(`列 ${key} 无 render`);
    return col.render(row);
  }

  it("row_descriptions_have_distinct_ids_and_their_own_content", async () => {
    mockOk();
    const settings = mount(Settings, { attachTo: document.body });
    await flushPromises();
    const host = mount(
      {
        setup: () => () => [
          renderCell(settings, "input", rowOf("vendor/alpha-model", "Alpha 对照")),
          renderCell(settings, "input", rowOf("vendor/beta-model", "Beta 对照")),
        ],
      },
      { attachTo: document.body },
    );
    const [ta, tb] = host.findAll("button.help-trigger");
    await ta!.trigger("focus");
    await waitTip(true);
    // 打开第二行后，两行各有自己的描述节点
    await tb!.trigger("focus");
    await flushPromises();
    const idA = ta!.attributes("aria-describedby");
    const idB = tb!.attributes("aria-describedby");
    expect(idA, "A 行应有描述关联").toBeTruthy();
    expect(idB, "B 行应有描述关联").toBeTruthy();
    expect(idA, "两行描述 id 必须互不相同").not.toBe(idB);
    const nodeA = document.getElementById(idA!);
    const nodeB = document.getElementById(idB!);
    expect(nodeA, "A 的描述节点必须存在").not.toBeNull();
    expect(nodeB, "B 的描述节点必须存在").not.toBeNull();
    // 两个 id 指向**不同**的节点，且各自内容属于本行（不串用那一份）
    expect(nodeA === nodeB, "两行不得指向同一个描述节点").toBe(false);
    expect(nodeA!.textContent).toContain("Alpha 对照");
    expect(nodeA!.textContent).not.toContain("Beta 对照");
    expect(nodeB!.textContent).toContain("Beta 对照");
    expect(nodeB!.textContent).not.toContain("Alpha 对照");
    // 触发器名称也各自带本行的模型前缀与列名
    expect(ta!.attributes("aria-label")).toContain("vendor/alpha-model 输入单价说明");
    expect(tb!.attributes("aria-label")).toContain("vendor/beta-model 输入单价说明");
    // 同一个 id 在文档里最多属于一份内容副本；两行之间绝不共用同一 id
    expect(document.querySelectorAll(`[id="${idA}"]`).length).toBeGreaterThan(0);
    host.unmount();
    settings.unmount();
  });
});

// ── RC07.4：真实调用点保留完整单价精度（不是只比元数据常量） ──
describe("Settings 单价精度真实渲染（RC07）", () => {
  it("unit_precision_survives_settings_and_breakdown", async () => {
    mockOk();
    const settings = mount(Settings, { attachTo: document.body });
    await flushPromises();
    const precise: PricingEntry = {
      ...entry,
      input: 1.234567,
      output: 12.3456789,
      cache_write: 0.123456789,
      cache_read: "same_as_input",
      openrouter: {
        input: 1.234567,
        output: 12.3456789,
        cache_write: 0,
        cache_read: null,
        name: "Precise",
      },
    };
    const cases: [string, string][] = [
      ["input", "$1.234567"],
      ["output", "$12.3456789"],
      ["cache_write", "$0.123456789"],
    ];
    for (const [key, expected] of cases) {
      const cell = mountCell(settings, key, precise);
      // 单元格**渲染文本**必须保留完整精度（此前被舍成 $1.23）
      expect(cell.text(), `${key} 列应显示 ${expected}`).toContain(expected);
      expect(cell.text()).not.toContain("$1.23 ");
      cell.unmount();
    }
    // SameAsInput 在单价视图仍保留语义，不被压成数字或未知
    const readCell = mountCell(settings, "cache_read", precise);
    expect(readCell.text()).toContain("同输入价");
    readCell.unmount();
    settings.unmount();
  });
});
