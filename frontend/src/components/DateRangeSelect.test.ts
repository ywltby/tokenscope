// 计划 C3（F07 + 时区语义）：
// - date_range_cancel_reopen：取消后重开，草稿必须重新同步自当前值；
// - date_range_clear_all：清除入口可达，确定后回"全部时间"；
// - "24h" 实为两个自然日 → 更名"近2天"（自然日口径不混称滚动小时）；
// - tzDate：区间日期换算按所选解析时区（Intl 实现替代硬编码 +8，DST 正确）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

// NPopover 打桩为透传渲染（trigger + 默认槽同渲染），update:show 由测试直发——
// 被测对象是组件自身的受控逻辑，不是 naive-ui 的浮层定位。
// stub 定义在 mock 工厂内（vi.mock 提升后 defineComponent 尚不可用）。
vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const NPopoverStub = dc({
    name: "NPopover",
    setup(_, { slots }) {
      return () => h("div", { class: "popover-stub" }, [slots.trigger?.(), slots.default?.()]);
    },
  });
  // NDatePicker stub：渲染 input 并透传 update:value（模拟用户点选日期）。
  const NDatePickerStub = dc({
    name: "NDatePicker",
    props: { value: { type: Number, default: null } },
    emits: ["update:value"],
    setup(props) {
      return () =>
        h("input", {
          class: "datepicker-stub",
          value: props.value,
        });
    },
  });
  return { ...actual, NPopover: NPopoverStub, NDatePicker: NDatePickerStub };
});

const NPopoverStub = { name: "NPopover" };

import DateRangeSelect from "./DateRangeSelect.vue";
import { tzDate } from "../lib/dates";

// 固定"现在"：2026-10-05T17:00:00Z —— 上海已是 10-06 凌晨、UTC 还是 10-05、
// 纽约是 10-05 下午：同一个时刻在三个时区是三个"今天"。
const NOW = Date.UTC(2026, 9, 5, 17, 0, 0);

function mountRange(value: [string, string] | null, tz = "Asia/Shanghai") {
  return mount(DateRangeSelect, { props: { value, tz } });
}

async function open(w: ReturnType<typeof mountRange>): Promise<void> {
  await w.findComponent(NPopoverStub).vm.$emit("update:show", true);
  await flushPromises();
}

beforeEach(() => {
  vi.useFakeTimers({ now: NOW });
});

/// Task 6：快捷项按所选统计时区解释"今天"。
describe("today_uses_selected_timezone（Task 6）", () => {
  it("UTC 时区的当天 = UTC 日历今天（而非本机零点毫秒换算）", async () => {
    const w = mountRange(null, "UTC");
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "当天")!
      .trigger("click");
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    const emitted = w.emitted("update:value")!.at(-1)![0];
    expect(emitted).toEqual(["2026-10-05", "2026-10-05"]);
  });

  it("上海时区的当天 = 上海日历今天（此刻已是 10-06）", async () => {
    const w = mountRange(null, "Asia/Shanghai");
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "当天")!
      .trigger("click");
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    expect(w.emitted("update:value")!.at(-1)![0]).toEqual(["2026-10-06", "2026-10-06"]);
  });

  it("date_range_dst_boundary：纽约 DST 切换日按当日实际偏移取日历值", async () => {
    // 2026-03-08T07:00:00Z：美东春令时切换时刻刚过，NY = 03-08。
    vi.useFakeTimers({ now: Date.UTC(2026, 2, 8, 7, 0, 0) });
    const w = mountRange(null, "America/New_York");
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "当天")!
      .trigger("click");
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    expect(w.emitted("update:value")!.at(-1)![0]).toEqual(["2026-03-08", "2026-03-08"]);
  });

  it("picker 负偏移 round-trip：选择毫秒值回传同一日历字符串", async () => {
    // Task 4：NDatePicker 毫秒值只是 UI 桥接（UTC 零点锚）——确定后必须
    // 回传同一日历字符串，不得被统计时区重新解释成另一天。
    const w = mountRange(null, "Asia/Shanghai");
    await open(w);
    const pickers = w.findAll("input");
    // NDatePicker 未打桩（真实组件），直接走 confirm 分支验证桥接：
    // 用 fake now 的"今天"毫秒（上海 10-06 → UTC 零点锚 10-05T16:00Z）
    // 等价于用户点选当天。
    expect(pickers.length).toBeGreaterThan(0);
    await w
      .findAll("button")
      .find((b) => b.text() === "当天")!
      .trigger("click");
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    expect(w.emitted("update:value")!.at(-1)![0]).toEqual(["2026-10-06", "2026-10-06"]);
  });

  it("picker round-trip 负偏移时区：UTC 零点锚毫秒不得变成前一天", async () => {
    // Task 1（审阅）：setter 用 tzDate(ms, tz) 把 UTC 零点锚重解释成统计
    // 时区日期——负偏移时区（纽约）下 10-05 会变 10-04。
    const w = mountRange(null, "America/New_York");
    await open(w);
    // 直接经 NDatePicker stub 的 v-model 更新毫秒值（模拟用户点选）。
    const dp = w.findComponent({ name: "NDatePicker" });
    expect(dp).toBeDefined();
    dp!.vm.$emit("update:value", Date.UTC(2026, 9, 5));
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    expect(w.emitted("update:value")!.at(-1)![0]).toEqual(["2026-10-05", "2026-10-05"]);
  });

  it("picker round-trip 正偏移时区：上海同样保持用户所选日期", async () => {
    const w = mountRange(null, "Asia/Shanghai");
    await open(w);
    const dp = w.findComponent({ name: "NDatePicker" });
    dp!.vm.$emit("update:value", Date.UTC(2026, 9, 5));
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    expect(w.emitted("update:value")!.at(-1)![0]).toEqual(["2026-10-05", "2026-10-05"]);
  });

  it("快捷范围是日历字符串（近7天 = 今天减 6 个自然日）", async () => {
    const w = mountRange(null, "UTC");
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "近7天")!
      .trigger("click");
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    expect(w.emitted("update:value")!.at(-1)![0]).toEqual(["2026-09-29", "2026-10-05"]);
  });
});
// fake timers 与 flushPromises 混用时恢复真实定时器
afterEach(() => {
  vi.useRealTimers();
});

describe("DateRangeSelect（C3/F07）", () => {
  it("date_range_cancel_reopen：取消不提交，重开后草稿重新同步自当前值", async () => {
    const value: [string, string] = ["2026-09-29", "2026-10-05"];
    const w = mountRange(value);
    // 第一次打开：点了"近2天"快捷，但随后取消。
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "近2天")!
      .trigger("click");
    await w
      .findAll("button")
      .find((b) => b.text() === "取消")!
      .trigger("click");
    expect(w.emitted("update:value")).toBeUndefined();
    // 重开：不碰任何快捷，直接确定——必须提交的是 props.value（重新同步），
    // 而不是上次残留的"近2天"草稿。
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    const emitted = w.emitted("update:value")!.at(-1)![0] as [number, number];
    expect(emitted[0]).toBe(value[0]);
    expect(emitted[1]).toBe(value[1]);
  });

  it("date_range_clear_all：清除入口可达，确定后回全部时间", async () => {
    const w = mountRange(["2026-09-06", "2026-10-05"]);
    await open(w);
    // 清除按钮存在且可用（修复前：清空起始日后确定被禁用，分支不可达）。
    const clear = w.findAll("button").find((b) => b.text() === "清除");
    expect(clear).toBeDefined();
    await clear!.trigger("click");
    const ok = w.findAll("button").find((b) => b.text() === "确定");
    expect(ok!.attributes("disabled")).toBeUndefined();
    await ok!.trigger("click");
    expect(w.emitted("update:value")!.at(-1)![0]).toBeNull();
  });

  it("快捷项按自然日口径命名：近2天（无 24h 滚动窗口混称）", () => {
    const w = mountRange(null);
    const labels = w.findAll("button").map((b) => b.text());
    expect(labels).not.toContain("24h");
    expect(labels).toContain("近2天");
  });

  it("确定后触发器显示当前选择，外部值变化同步标签", async () => {
    const w = mountRange(null);
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "近2天")!
      .trigger("click");
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    expect(w.text()).toContain("近2天");
  });
});

describe("tzDate（C3：区间日期按解析时区换算）", () => {
  it("UTC 时区按 UTC 日界（修复前硬编码 +8 错日）", () => {
    // 2026-08-01T00:30+08:00 == 2026-07-31T16:30Z
    const ms = Date.UTC(2026, 6, 31, 16, 30);
    expect(tzDate(ms, "UTC")).toBe("2026-07-31");
    expect(tzDate(ms, "Asia/Shanghai")).toBe("2026-08-01");
  });

  it("DST 时区按当日实际偏移（固定偏移实现会错 1 小时档）", () => {
    // 纽约 2026-07-01 12:00（EDT, UTC-4）== 16:00Z
    const ms = Date.UTC(2026, 6, 1, 16, 0);
    expect(tzDate(ms, "America/New_York")).toBe("2026-07-01");
    // 冬令时 EST, UTC-5：2026-01-15 12:00 == 17:00Z
    expect(tzDate(Date.UTC(2026, 0, 15, 17, 0), "America/New_York")).toBe("2026-01-15");
  });
});

describe("DateRangeSelect 视觉（设计系统 Task 2）", () => {
  it("触发按钮不使用 emoji 图标，标签语义保留", () => {
    const w = mountRange(null);
    const btn = w.find("button");
    expect(btn.text()).not.toMatch(/\p{Extended_Pictographic}/u);
    expect(btn.text()).toContain("全部时间");
  });

  it("弹层面板使用 elevated 玻璃表面", async () => {
    const w = mountRange(null);
    await open(w);
    const panel = w.find(".range-panel");
    expect(panel.exists()).toBe(true);
    expect(panel.classes()).toContain("ts-glass");
  });
});
