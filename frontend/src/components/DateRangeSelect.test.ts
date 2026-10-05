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
  return { ...actual, NPopover: NPopoverStub };
});

const NPopoverStub = { name: "NPopover" };

import DateRangeSelect from "./DateRangeSelect.vue";
import { tzDate } from "../lib/dates";

const DAY = 86400e3;
// 固定"今天"：2026-08-05 上海时间 10:00（= 08-05T02:00Z）。
const NOW = Date.UTC(2026, 7, 5, 2, 0, 0);
const todayStart = Date.UTC(2026, 7, 4, 16, 0, 0); // 上海 08-05 00:00

function mountRange(value: [number, number] | null) {
  return mount(DateRangeSelect, { props: { value } });
}

async function open(w: ReturnType<typeof mountRange>): Promise<void> {
  await w.findComponent(NPopoverStub).vm.$emit("update:show", true);
  await flushPromises();
}

beforeEach(() => {
  vi.useFakeTimers({ now: NOW });
});
// fake timers 与 flushPromises 混用时恢复真实定时器
afterEach(() => {
  vi.useRealTimers();
});

describe("DateRangeSelect（C3/F07）", () => {
  it("date_range_cancel_reopen：取消不提交，重开后草稿重新同步自当前值", async () => {
    const value: [number, number] = [todayStart - 6 * DAY, todayStart + DAY - 1];
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
    const w = mountRange([todayStart - 29 * DAY, todayStart + DAY - 1]);
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
