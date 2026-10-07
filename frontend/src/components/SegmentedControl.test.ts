// UX02：分段控件名称、焦点与真实尺寸。
// 方向键必须真实移动 DOM 焦点（修复前只 emit）；底槽外高 32px；thumb 跟随
// 选中项尺寸变化；外部改值不抢焦点。happy-dom 无布局，getBoundingClientRect
// 打桩为确定性矩形，ResizeObserver 打桩为可控回调。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, nextTick, ref } from "vue";
import SegmentedControl from "./SegmentedControl.vue";

const roCallbacks: (() => void)[] = [];
vi.stubGlobal(
  "ResizeObserver",
  class {
    constructor(cb: () => void) {
      roCallbacks.push(cb);
    }
    observe() {}
    unobserve() {}
    disconnect() {}
  },
);

let itemWidths = [60, 50, 80];
const rect = (left: number, top: number, width: number, height: number): DOMRect =>
  ({
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
    toJSON: () => ({}),
  }) as DOMRect;

beforeEach(() => {
  roCallbacks.length = 0;
  itemWidths = [60, 50, 80];
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.classList?.contains("ts-segmented")) return rect(100, 10, 200, 32);
    if (this.classList?.contains("ts-segmented-item")) {
      const parent = this.parentElement;
      const idx = parent
        ? Array.from(parent.querySelectorAll(".ts-segmented-item")).indexOf(this)
        : 0;
      let left = 102;
      for (let i = 0; i < idx; i++) left += itemWidths[i] + 2;
      return rect(left, 12, itemWidths[idx] ?? 0, 28);
    }
    return rect(0, 0, 0, 0);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

const OPTIONS = [
  { value: "a", label: "A" },
  { value: "b", label: "B" },
  { value: "c", label: "C" },
] as const;

function mountCtl(modelValue: "a" | "b" | "c" = "a") {
  return mount(SegmentedControl, {
    props: { modelValue, options: [...OPTIONS], ariaLabel: "测试分组" },
    attachTo: document.body,
  });
}

/// v-model 宿主：让 aria-checked 随选择真实更新（纯 mount 不会回写 props）。
function mountVModel(initial: "a" | "b" | "c" = "a") {
  const value = ref<"a" | "b" | "c">(initial);
  const w = mount(
    defineComponent({
      setup() {
        return () =>
          h(SegmentedControl, {
            modelValue: value.value,
            options: [...OPTIONS],
            ariaLabel: "测试分组",
            "onUpdate:modelValue": (v: string) => {
              value.value = v as "a" | "b" | "c";
            },
          });
      },
    }),
    { attachTo: document.body },
  );
  return w;
}

describe("SegmentedControl（UX02）", () => {
  it("arrow_selection_moves_dom_focus：方向键移动选中并真实聚焦", async () => {
    const w = mountVModel("a");
    await flushPromises();
    const items = w.findAll(".ts-segmented-item");
    (items[0].element as HTMLElement).focus();
    expect(document.activeElement).toBe(items[0].element);

    await items[0].trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    // 选中值与 aria-checked 更新
    expect(items[1].attributes("aria-checked")).toBe("true");
    expect(items[0].attributes("aria-checked")).toBe("false");
    // 焦点真实移动到新选中项（修复前焦点留在原按钮）
    expect(document.activeElement).toBe(items[1].element);

    await items[1].trigger("keydown", { key: "ArrowLeft" });
    await flushPromises();
    expect(document.activeElement).toBe(items[0].element);

    // 环绕：从第一项再向左回到最后一项
    await items[0].trigger("keydown", { key: "ArrowLeft" });
    await flushPromises();
    expect(document.activeElement).toBe(items[2].element);
    w.unmount();
  });

  it("tab_enters_selected_radio_and_leaves_group：仅选中项可 Tab 进入", async () => {
    const w = mountCtl("b");
    await flushPromises();
    const items = w.findAll(".ts-segmented-item");
    expect(items.map((i) => i.attributes("tabindex"))).toEqual(["-1", "0", "-1"]);
    // 组内只有选中项在 Tab 序列中 → Tab 进入选中项，再按 Tab 离开整组
    (items[1].element as HTMLElement).focus();
    expect(document.activeElement).toBe(items[1].element);
    w.unmount();
  });

  it("theme_radios_have_semantic_names：主题项有语义名称，装饰图形对读屏隐藏", async () => {
    const w = mount(SegmentedControl, {
      props: {
        modelValue: "system",
        ariaLabel: "主题偏好",
        options: [
          { value: "light", label: "浅色", ariaLabel: "浅色模式" },
          { value: "dark", label: "深色", ariaLabel: "深色模式" },
          { value: "system", label: "跟随系统", ariaLabel: "跟随系统" },
        ],
      },
      slots: {
        icon: ({ option }: { option: { value: string } }) =>
          h("svg", { class: `theme-icon-${option.value}`, "data-test": "icon" }),
      },
    });
    await flushPromises();
    const items = w.findAll(".ts-segmented-item");
    expect(items.map((i) => i.attributes("aria-label"))).toEqual([
      "浅色模式",
      "深色模式",
      "跟随系统",
    ]);
    // 装饰图标容器 aria-hidden，真实名称由按钮承载
    for (const i of items) {
      const wrap = i.find(".seg-icon");
      expect(wrap.exists()).toBe(true);
      expect(wrap.attributes("aria-hidden")).toBe("true");
    }
    w.unmount();
  });

  it("resizing_selected_option_repositions_thumb：选中项尺寸变化重定位 thumb", async () => {
    const w = mountCtl("a");
    await flushPromises();
    const thumb = w.find(".ts-segmented-thumb").element as HTMLElement;
    expect(thumb.style.width).toBe("60px");
    expect(thumb.style.transform).toBe("translateX(2px)");

    // 选中项被撑宽（内容变化/字体加载）：ResizeObserver 触发重定位
    itemWidths = [90, 50, 80];
    for (const cb of roCallbacks) cb();
    await nextTick();
    await flushPromises();
    expect(thumb.style.width).toBe("90px");
    expect(thumb.style.transform).toBe("translateX(2px)");
    w.unmount();
  });

  it("external_selection_does_not_steal_focus：外部改值只重定位不抢焦点", async () => {
    const w = mountCtl("a");
    await flushPromises();
    const items = w.findAll(".ts-segmented-item");
    (items[0].element as HTMLElement).focus();
    expect(document.activeElement).toBe(items[0].element);

    await w.setProps({ modelValue: "c" });
    await flushPromises();
    // 焦点未被外部改值抢走
    expect(document.activeElement).toBe(items[0].element);
    // thumb 已跟随新选中项
    const thumb = w.find(".ts-segmented-thumb").element as HTMLElement;
    expect(thumb.style.transform).toBe("translateX(116px)");
    w.unmount();
  });

  it("空 options 安全无动作", async () => {
    const w = mount(SegmentedControl, {
      props: { modelValue: "a", options: [], ariaLabel: "空" },
    });
    await flushPromises();
    await w.find(".ts-segmented").trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    expect(w.emitted("update:modelValue")).toBeUndefined();
    w.unmount();
  });
});
