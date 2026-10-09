// 设计系统 Task 4 + UX05：TrendChart 主题接入、高度上限、可访问摘要，
// 以及实例生命周期与数据更新分离（不因无关父级更新重建；同维度 merge 保留
// zoom；维度切换完整替换；主题切换一次 dispose/init；0 类别不建实例）。
// echarts 全 mock（happy-dom 无 canvas），ResizeObserver 打桩。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, nextTick, ref } from "vue";

const echartsMock = vi.hoisted(() => {
  const setOption = vi.fn();
  const dispose = vi.fn();
  const resize = vi.fn();
  const getOption = vi.fn((): unknown => ({}));
  const dispatchAction = vi.fn();
  const init = vi.fn((..._args: unknown[]) => ({
    setOption,
    dispose,
    resize,
    getOption,
    dispatchAction,
  }));
  return { setOption, dispose, resize, getOption, dispatchAction, init };
});

vi.mock("echarts", () => ({ init: echartsMock.init }));

vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    disconnect() {}
    unobserve() {}
  },
);

import TrendChart from "./TrendChart.vue";
import { chartTokens } from "../styles/chartTheme";
import { useTheme } from "../composables/theme";
import { useTokenColors } from "../composables/tokenColors";
import type { Group } from "../types";

function group(key: string, input: number): Group {
  return {
    key,
    label: key.split("/").pop() ?? key,
    requests: 1,
    tokens: { input, output: 0, cache_write: 0, cache_read: 0 },
    cost_usd: 0,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  };
}

const setOption = echartsMock.setOption;
const init = echartsMock.init;
const dispose = echartsMock.dispose;

beforeEach(() => {
  useTokenColors().resetColors();
  setOption.mockClear();
  dispose.mockClear();
  echartsMock.resize.mockClear();
  echartsMock.getOption.mockClear();
  echartsMock.getOption.mockReturnValue({});
  init.mockClear();
  localStorage.removeItem("tokenscope-theme");
  useTheme().setPreference("system");
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe("TrendChart（设计系统 Task 4）", () => {
  it.each([
    ["day", "使用趋势"],
    ["model", "模型用量"],
    ["agent", "应用用量"],
    ["project", "项目用量"],
  ])("%s 使用对应标题", async (by, title) => {
    const w = mount(TrendChart, { props: { groups: [group("a", 10)], by } });
    expect(w.find(".chart-title").text()).toBe(title);
    expect(w.find(".chart-canvas").attributes("aria-label")).toBe(title);
    w.unmount();
  });
  it("刷新和主题切换保留缩放；恢复按钮回到完整范围；普通滚轮不缩放", async () => {
    const w = mount(TrendChart, {
      props: { groups: [group("a", 10), group("b", 20)], by: "model" },
    });
    const initial = setOption.mock.calls[0][0];
    expect(initial.dataZoom[1]).toMatchObject({
      zoomOnMouseWheel: "ctrl",
      moveOnMouseWheel: false,
    });
    echartsMock.getOption.mockReturnValue({ dataZoom: [{ start: 25, end: 75 }] });
    await w.setProps({ groups: [group("a", 15), group("b", 25)] });
    expect(setOption.mock.calls.at(-1)![0].dataZoom).toBeUndefined();
    useTheme().setPreference("dark");
    await nextTick();
    expect(setOption.mock.calls.at(-1)![0].dataZoom[0]).toMatchObject({ start: 25, end: 75 });
    await w.find(".chart-reset").trigger("click");
    expect(echartsMock.dispatchAction).toHaveBeenCalledWith({
      type: "dataZoom",
      start: 0,
      end: 100,
    });
    await w.setProps({ by: "day" });
    expect(setOption.mock.calls.at(-1)![0].dataZoom[0]).toMatchObject({
      start: 0,
      end: 100,
      xAxisIndex: 0,
    });
    w.unmount();
  });
  it.each(["day", "model", "project", "agent"])("%s 超过200类仍保留每类的四项数据", async (by) => {
    const groups = Array.from({ length: 240 }, (_, i) => ({
      ...group(`category-${i}`, i),
      tokens: { input: i, output: i * 2, cache_write: i * 3, cache_read: i * 4 },
    }));
    const w = mount(TrendChart, { props: { groups, by } });
    await flushPromises();
    const opt = setOption.mock.calls[0][0];
    expect(opt.dataZoom[0]).toMatchObject({ start: 0, end: 100 });
    expect(opt.series).toHaveLength(4);
    for (const [index, series] of opt.series.entries()) {
      expect(series.data).toHaveLength(240);
      expect(series.data.reduce((a: number, b: number) => a + b, 0)).toBe(
        ((239 * 240) / 2) * (index + 1),
      );
    }
    if (by === "day")
      expect(Number.parseInt((w.find(".chart-canvas").element as HTMLElement).style.minWidth)).toBe(
        0,
      );
    await w.setProps({ groups: groups.slice(0, 2) });
    expect(
      setOption.mock.calls.at(-1)![0].series.every((s: { data: number[] }) => s.data.length === 2),
    ).toBe(true);
    w.unmount();
  });
  it("颜色修改同时更新系列与图例，不重建图表或覆盖缩放", async () => {
    const w = mount(TrendChart, { props: { groups: [group("a", 1)], by: "model" } });
    await flushPromises();
    setOption.mockClear();
    useTokenColors().setColor("cache_read", "#AA2266");
    await nextTick();
    expect(init).toHaveBeenCalledTimes(1);
    expect(dispose).not.toHaveBeenCalled();
    expect(setOption).toHaveBeenLastCalledWith({
      color: ["#4C8DF6", "#F2A24A", "#5BBF7A", "#AA2266"],
    });
    expect(w.findAll(".legend-dot")[3].attributes("style")).toContain("#AA2266");
    w.unmount();
  });
  it("不使用默认调色板：series 颜色来自 chartTokens 固定语义色", async () => {
    const w = mount(TrendChart, { props: { groups: [group("a", 1)], by: "model" } });
    await flushPromises();
    expect(setOption).toHaveBeenCalled();
    const opt = setOption.mock.calls[0][0] as { color: string[]; backgroundColor: string };
    const expected = chartTokens("light").series.map((s) => s.color);
    expect(opt.color).toEqual(expected);
    expect(opt.backgroundColor).toBe("transparent");
    w.unmount();
  });

  it.each(["model", "project", "agent"])(
    "%s 保留全部类别且画布高度封顶，提供显式缩放",
    async (by) => {
      const many = Array.from({ length: 50 }, (_, i) => group(`model-${i}`, 50 - i));
      const w = mount(TrendChart, { props: { groups: many, by } });
      await flushPromises();
      const opt = setOption.mock.calls[0][0] as {
        dataZoom?: unknown[];
        yAxis: { data: string[] };
      };
      expect(opt.dataZoom).toHaveLength(2);
      expect(opt.yAxis.data).toHaveLength(50);
      const h2 = Number.parseInt((w.find(".chart-canvas").element as HTMLElement).style.height, 10);
      expect(h2).toBeLessThanOrEqual(480);
      expect(init.mock.calls[0][2]).toEqual({ renderer: "svg" });
      w.unmount();
    },
  );

  it("保留图表画布，不提供数据摘要面板", async () => {
    const w = mount(TrendChart, {
      props: { groups: [group("alpha", 100), group("beta", 40)], by: "model" },
    });
    await flushPromises();
    expect(w.find(".chart-canvas").exists()).toBe(true);
    expect(w.find(".summary-toggle").exists()).toBe(false);
    expect(w.find(".chart-summary").exists()).toBe(false);
    w.unmount();
  });

  it("主题切换时销毁并重建实例一次（不残留旧主题）", async () => {
    const w = mount(TrendChart, { props: { groups: [group("a", 1)], by: "day" } });
    await flushPromises();
    expect(init).toHaveBeenCalledTimes(1);
    const disposeBefore = dispose.mock.calls.length;
    useTheme().setPreference("dark");
    await nextTick();
    await flushPromises();
    // 主题切换：一次 dispose + 一次 init（重建，不残留旧主题轴/文字色）
    expect(dispose.mock.calls.length).toBe(disposeBefore + 1);
    expect(init).toHaveBeenCalledTimes(2);
    const last = setOption.mock.calls.at(-1)![0] as { textStyle?: unknown; color: string[] };
    expect(last.color).toEqual(chartTokens("dark").series.map((s) => s.color));
    useTheme().setPreference("system");
    w.unmount();
  });
});

describe("TrendChart 卡片化（设计系统 Task 5）", () => {
  it("外层 .ts-card，图例移到卡片头部（HTML 圆点图例），画布内不再渲染图例", async () => {
    const w = mount(TrendChart, { props: { groups: [group("a", 1)], by: "day" } });
    await flushPromises();
    expect(w.find("section.ts-card").exists()).toBe(true);
    const legend = w.find(".chart-legend");
    expect(legend.exists()).toBe(true);
    expect(legend.findAll(".legend-item").length).toBe(4);
    const opt = setOption.mock.calls[0][0] as { legend?: unknown };
    expect(opt.legend).toBeUndefined();
    w.unmount();
  });

  it("堆叠柱仅最上段系列带圆角：日维度柱顶 [4,4,0,0]，非日维度条尾 [0,4,4,0]", async () => {
    const wDay = mount(TrendChart, { props: { groups: [group("a", 1)], by: "day" } });
    await flushPromises();
    const optDay = setOption.mock.calls[0][0] as {
      series: { itemStyle?: { borderRadius?: number[] } }[];
    };
    expect(optDay.series).toHaveLength(4);
    expect(optDay.series[3].itemStyle?.borderRadius).toEqual([4, 4, 0, 0]);
    for (const s of optDay.series.slice(0, 3)) {
      expect(s.itemStyle?.borderRadius).toBeUndefined();
    }
    wDay.unmount();

    const wModel = mount(TrendChart, { props: { groups: [group("a", 1)], by: "model" } });
    await flushPromises();
    const optModel = setOption.mock.calls.at(-1)![0] as {
      series: { itemStyle?: { borderRadius?: number[] } }[];
    };
    expect(optModel.series[3].itemStyle?.borderRadius).toEqual([0, 4, 4, 0]);
    wModel.unmount();
  });
});

describe("TrendChart 生命周期与数据更新（UX05）", () => {
  it("unrelated_parent_update_does_not_touch_chart：无关父级更新不触发 init/dispose/setOption", async () => {
    const tick = ref(0);
    const groups = ref<Group[]>([group("a", 1)]);
    const w = mount(
      defineComponent({
        setup: () => () =>
          h("div", [
            h(TrendChart, { groups: groups.value, by: "day" }),
            h("i", { class: "tick" }, String(tick.value)),
          ]),
      }),
    );
    await flushPromises();
    expect(init).toHaveBeenCalledTimes(1);
    const setOptionCount = setOption.mock.calls.length;
    const disposeCount = dispose.mock.calls.length;

    tick.value = 1; // 无关状态变化 → 父级重渲染，但 groups 身份不变
    await nextTick();
    await flushPromises();

    expect(init).toHaveBeenCalledTimes(1);
    expect(dispose.mock.calls.length).toBe(disposeCount);
    expect(setOption.mock.calls.length).toBe(setOptionCount);
    expect(w.find(".tick").text()).toBe("1");
    w.unmount();
  });

  it("new_groups_update_existing_instance：新数据更新现有实例（merge，不重建）", async () => {
    const w = mount(TrendChart, { props: { groups: [group("a", 1)], by: "model" } });
    await flushPromises();
    expect(init).toHaveBeenCalledTimes(1);
    const disposeCount = dispose.mock.calls.length;
    setOption.mockClear();

    await w.setProps({ groups: [group("a", 9), group("b", 5)] });
    await flushPromises();

    // 不重建实例
    expect(init).toHaveBeenCalledTimes(1);
    expect(dispose.mock.calls.length).toBe(disposeCount);
    // 数据更新走 setOption(merge)
    expect(setOption).toHaveBeenCalledTimes(1);
    expect(setOption.mock.calls[0][1]).toEqual({ notMerge: false });
    const opt = setOption.mock.calls[0][0] as {
      series: { id: string; data: number[] }[];
      yAxis: { data: string[] };
    };
    expect(opt.yAxis.data).toEqual(["a", "b"]);
    expect(opt.series[0].data).toEqual([9, 5]);
    w.unmount();
  });

  it("dimension_switch_removes_obsolete_axes：维度切换完整替换 option", async () => {
    const w = mount(TrendChart, { props: { groups: [group("2026-10-01", 1)], by: "day" } });
    await flushPromises();
    setOption.mockClear();
    const viewport = w.find(".chart-body").element;
    viewport.scrollLeft = 200;
    viewport.scrollTop = 300;

    await w.setProps({ groups: [group("m1", 3)], by: "model" });
    await flushPromises();

    expect(setOption).toHaveBeenCalledTimes(1);
    // notMerge:true → 清掉旧轴 / series / dataZoom 残留
    expect(setOption.mock.calls[0][1]).toEqual({ notMerge: true });
    const opt = setOption.mock.calls[0][0] as { xAxis: { type: string }; yAxis: { type: string } };
    // 日维度 x=category/y=value；模型维度相反（x=value/y=category）
    expect(opt.xAxis.type).toBe("value");
    expect(opt.yAxis.type).toBe("category");
    expect(viewport.scrollTop).toBe(0);
    expect(viewport.scrollLeft).toBe(0);
    w.unmount();
  });

  it("同维度刷新沿用实例并完整更新全部数据", async () => {
    const many = Array.from({ length: 20 }, (_, i) => group(`model-${i}`, 20 - i));
    const w = mount(TrendChart, { props: { groups: many, by: "model" } });
    await flushPromises();
    expect(init).toHaveBeenCalledTimes(1);
    const disposeCount = dispose.mock.calls.length;
    setOption.mockClear();

    await w.setProps({ groups: many.map((g2, i) => group(g2.key, i)) });
    await flushPromises();

    expect(init).toHaveBeenCalledTimes(1);
    expect(dispose.mock.calls.length).toBe(disposeCount);
    expect(setOption.mock.calls[0][1]).toEqual({ notMerge: false });
    w.unmount();
  });

  it("0 类别不初始化实例（父级另给明确空状态）", async () => {
    const w = mount(TrendChart, { props: { groups: [], by: "model" } });
    await flushPromises();
    expect(init).not.toHaveBeenCalled();
    expect(w.find(".chart-state").exists()).toBe(false);
    w.unmount();
  });

  it("unmount 释放实例（dispose 调用一次）", async () => {
    const w = mount(TrendChart, { props: { groups: [group("a", 1)], by: "day" } });
    await flushPromises();
    w.unmount();
    expect(dispose).toHaveBeenCalledTimes(1);
  });

  it("SF01 回归：所有 setOption 路径的 tooltip 仍以 textContent 输出不可信标签", async () => {
    const payload = `<img src=x onerror=alert(1)>sentinel-9f3`;
    const w = mount(TrendChart, { props: { groups: [group(payload, 7)], by: "model" } });
    await flushPromises();
    const opt = setOption.mock.calls[0][0] as {
      tooltip: { formatter: (p: unknown) => HTMLElement };
    };
    const node = opt.tooltip.formatter([
      { dataIndex: 0, name: payload, seriesName: "输入", value: 7 },
    ]);
    expect(node).toBeInstanceOf(HTMLElement);
    expect(node.querySelector("img")).toBeNull();
    expect(node.textContent).toContain(payload);

    // 同维度数据更新路径（merge）也必须使用同一安全 formatter
    setOption.mockClear();
    await w.setProps({ groups: [group(payload, 8), group("b", 1)] });
    await flushPromises();
    const opt2 = setOption.mock.calls.at(-1)![0] as {
      tooltip: { formatter: (p: unknown) => HTMLElement };
    };
    const node2 = opt2.tooltip.formatter([
      { dataIndex: 0, name: payload, seriesName: "输入", value: 8 },
    ]);
    expect(node2.querySelector("img")).toBeNull();
    expect(node2.textContent).toContain(payload);
    w.unmount();
  });
});
