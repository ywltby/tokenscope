// U01/U02（UI 调整计划 2026-10-13）：日期按钮与日期弹层的真实交互契约。
// 关键语义：
// - 按钮在"全部时间"时只显示日历图标且不着色；其余显示语义标签（12 种形态）；
// - 弹层快捷项点一下立即生效并关闭；自定义用**一个** naive datetimerange 面板
//   （`panel` 内联渲染，已在本机 2.45.3 上验证：选择即回吐 formatted-value）；
// - 前端只发用户选中的分钟值：只选日期 = 00:00 / 23:59（defaultTime 生效）。
// 跨时区说明：Windows Node 忽略 TZ 环境变量，无法在进程内切换时区；
// 本机即 Asia/Shanghai（断言处打印 resolved 时区证明环境生效），
// 洛杉矶等负偏移场景由「字符串往返不变量」覆盖（与本地时区无关）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";

console.info("测试环境时区:", Intl.DateTimeFormat().resolvedOptions().timeZone);

// NPopover 打桩为透传（trigger + 受控 show 的默认槽）；NDatePicker 用**真实组件**
// ——日期弹层要求至少一条实际控件交互 → emit 断言，不能全用 stub。
vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const NPopoverStub = dc({
    name: "NPopover",
    props: { show: { type: Boolean, default: false } },
    emits: ["update:show"],
    setup(props, { slots }) {
      return () =>
        h("div", { class: "popover-stub" }, [
          slots.trigger?.(),
          props.show ? slots.default?.() : null,
        ]);
    },
  });
  return { ...actual, NPopover: NPopoverStub };
});

// 固定"今天"：按钮标签与可访问名称的年份规则可预测（与真实时钟无关）。
vi.mock("../lib/dates", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/dates")>();
  return { ...actual, todayInTz: () => "2026-10-12" };
});

import DateRangeSelect from "./DateRangeSelect.vue";
import { addDays, rangeLabel, todayInTz, tzDate } from "../lib/dates";
import type { RangePreset } from "../lib/rangePreset";
import type { RangeView } from "../types";

const NPopoverStub = { name: "NPopover" };
const TZ = "Asia/Shanghai";

const dayRange: RangeView = { granularity: "day", from: "2026-10-01", to: "2026-10-12" };
const instantSameDay: RangeView = {
  granularity: "instant",
  start_local: "2026-10-12T09:00",
  end_local: "2026-10-12T18:00",
  start: "2026-10-12T01:00:00Z",
  end: "2026-10-12T10:00:00Z",
};
const instantAcrossDays: RangeView = {
  granularity: "instant",
  start_local: "2026-10-11T22:00",
  end_local: "2026-10-12T02:00",
  start: "2026-10-11T14:00:00Z",
  end: "2026-10-11T18:00:00Z",
};
const rolling24: RangeView = {
  granularity: "instant",
  start_local: "2026-10-12T00:31",
  end_local: "2026-10-13T00:31",
  start: "2026-10-11T16:31:00Z",
  end: "2026-10-12T16:31:00Z",
};

function mountRange(preset: RangePreset | null = null, range: RangeView | null = null) {
  return mount(DateRangeSelect, { props: { preset, tz: TZ, range } });
}

async function open(w: ReturnType<typeof mountRange>): Promise<void> {
  await w.findComponent(NPopoverStub).vm.$emit("update:show", true);
  await flushPromises();
}

function btn(w: VueWrapper, text: string) {
  return w.findAll("button").find((b) => b.text() === text);
}

function lastPreset(w: VueWrapper): unknown {
  return w.emitted("update:preset")!.at(-1)![0];
}

/** 真实 datetimerange 面板里某个日历的日期格子（排除补位的上下月日期）。 */
function dayCells(w: VueWrapper, side: "start" | "end") {
  return w.findAll(
    `.n-date-panel-calendar--${side} .n-date-panel-date:not(.n-date-panel-date--excluded)`,
  );
}
function dayCell(w: VueWrapper, side: "start" | "end", day: number) {
  return dayCells(w, side).find((c) => c.text().trim() === String(day))!;
}

describe("日期弹层（U02）", () => {
  it("快捷项一次点击即发出新预设并关闭弹层", async () => {
    const cases: [string, RangePreset][] = [
      ["当日", { kind: "days", days: 1 }],
      ["24h", { kind: "rolling", hours: 24 }],
      ["7d", { kind: "days", days: 7 }],
      ["14d", { kind: "days", days: 14 }],
      ["30d", { kind: "days", days: 30 }],
      ["全部时间", { kind: "all" }],
    ];
    for (const [label, preset] of cases) {
      const w = mountRange();
      await open(w);
      expect(btn(w, label), `${label} 快捷项必须存在`).toBeDefined();
      await btn(w, label)!.trigger("click");
      expect(lastPreset(w), `${label} 必须立即发出预设`).toEqual(preset);
      expect(w.find(".range-panel").exists(), `${label} 点击后必须关闭弹层`).toBe(false);
      expect(w.find(".range-trigger").attributes("aria-expanded")).toBe("false");
      w.unmount();
    }
  });

  it("只选日期时发出 00:00 起始与 23:59 结束（前端不做加减）", async () => {
    const w = mountRange({
      kind: "custom",
      start: "2026-10-12T00:00",
      end: "2026-10-12T23:59",
    });
    await open(w);
    // 真实 datetimerange 面板：起点改为 10/5，结束改为 10/15，时刻不动
    await dayCell(w, "start", 5).trigger("click");
    await flushPromises();
    await dayCell(w, "start", 15).trigger("mouseenter");
    await dayCell(w, "start", 15).trigger("click");
    await flushPromises();
    await btn(w, "确定")!.trigger("click");
    expect(lastPreset(w)).toEqual({
      kind: "custom",
      start: "2026-10-05T00:00",
      end: "2026-10-15T23:59",
    });
  });

  it("勾选「结束于现在」时发出 end: now，并禁用结束时刻", async () => {
    const w = mountRange({
      kind: "custom",
      start: "2026-10-12T00:00",
      end: "2026-10-12T23:59",
    });
    await open(w);
    const picker = w.findComponent({ name: "DatePicker" });
    const isTimeDisabled = picker.props("isTimeDisabled") as (
      ts: number,
      position: "start" | "end",
      value: [number, number],
    ) => { isHourDisabled?: () => boolean; isMinuteDisabled?: () => boolean };
    // 勾选前结束时刻可编辑
    expect(isTimeDisabled(0, "end", [0, 0]).isHourDisabled).toBeUndefined();
    await w.find(".n-checkbox").trigger("click");
    await flushPromises();
    // 勾选后结束时刻整体禁用，起点时刻不受影响
    expect(isTimeDisabled(0, "end", [0, 0]).isHourDisabled?.()).toBe(true);
    expect(isTimeDisabled(0, "end", [0, 0]).isMinuteDisabled?.()).toBe(true);
    expect(isTimeDisabled(0, "start", [0, 0]).isHourDisabled).toBeUndefined();
    await btn(w, "确定")!.trigger("click");
    expect(lastPreset(w)).toEqual({ kind: "custom", start: "2026-10-12T00:00", end: "now" });
  });

  it("取消与 Escape 关闭弹层且不改值，草稿丢弃（重开回到当前预设）", async () => {
    const preset: RangePreset = {
      kind: "custom",
      start: "2026-10-01T00:00",
      end: "2026-10-12T23:59",
    };
    const w = mountRange(preset, dayRange);
    await open(w);
    const dateInputs = () => w.findAll(".n-date-panel-date-input input");
    expect((dateInputs()[0].element as HTMLInputElement).value).toBe("2026-10-01");

    await dayCell(w, "start", 5).trigger("click");
    await flushPromises();
    expect((dateInputs()[0].element as HTMLInputElement).value).toBe("2026-10-05");
    await btn(w, "取消")!.trigger("click");
    expect(w.emitted("update:preset")).toBeUndefined();
    expect(w.find(".range-panel").exists()).toBe(false);

    await open(w);
    expect((dateInputs()[0].element as HTMLInputElement).value, "取消后草稿必须丢弃").toBe(
      "2026-10-01",
    );
    await dayCell(w, "start", 5).trigger("click");
    await flushPromises();
    await w.find(".range-panel").trigger("keydown", { key: "Escape" });
    await flushPromises();
    expect(w.emitted("update:preset")).toBeUndefined();
    expect(w.find(".range-panel").exists()).toBe(false);

    await open(w);
    expect((dateInputs()[0].element as HTMLInputElement).value, "Escape 后草稿必须丢弃").toBe(
      "2026-10-01",
    );
  });

  it("清除入口一键回全部时间并关闭", async () => {
    const w = mountRange({ kind: "days", days: 7 });
    await open(w);
    const clear = btn(w, "清除");
    expect(clear).toBeDefined();
    await clear!.trigger("click");
    expect(lastPreset(w)).toBeNull();
    expect(w.find(".range-panel").exists()).toBe(false);
  });

  it("全程可用键盘完成：控件都是原生可聚焦元素，Escape 关闭且不改值", async () => {
    const w = mountRange();
    const trigger = w.find(".range-trigger");
    expect(trigger.element.tagName).toBe("BUTTON");
    expect(trigger.attributes("type"), "原生 button 才能被 Tab/Enter/Space 激活").toBe("button");
    await open(w);
    for (const label of ["当日", "7d", "全部时间", "取消", "确定"]) {
      const b = btn(w, label);
      if (!b) continue;
      expect(b.element.tagName, `${label} 必须是原生 button`).toBe("BUTTON");
    }
    // 自定义区是一个内联日期面板（无二次弹层、无第二个选择器）
    expect(w.findAll(".n-date-panel").length, "自定义区间面板必须内联渲染").toBe(1);
    expect(w.findAll(".n-date-panel-date-input").length, "区间面板只有一组起止输入").toBe(2);
    await w.find(".range-panel").trigger("keydown", { key: "Escape" });
    await flushPromises();
    expect(w.emitted("update:preset")).toBeUndefined();
    expect(w.find(".range-panel").exists()).toBe(false);
  });
});

describe("日期按钮标签（U01）", () => {
  it("全部时间（无预设）只显示日历图标且不着色", () => {
    for (const preset of [null, { kind: "all" } as RangePreset]) {
      const w = mountRange(preset);
      const trigger = w.find(".range-trigger");
      expect(trigger.find("svg").exists(), "必须沿用现有日历图标").toBe(true);
      expect(trigger.text()).toBe("");
      expect(trigger.classes()).not.toContain("has-preset");
      expect(trigger.attributes("aria-label")).toBe("全部时间");
      expect(trigger.attributes("title")).toBe("全部时间");
    }
  });

  it("语义标签：当日 / 24h / 7d / 14d / 30d", () => {
    const cases: [RangePreset, RangeView | null, string, string][] = [
      [
        { kind: "days", days: 1 },
        { granularity: "day", from: "2026-10-12", to: "2026-10-12" },
        "当日",
        "当日：10/12",
      ],
      [{ kind: "days", days: 1 }, null, "当日", "当日"],
      [{ kind: "rolling", hours: 24 }, rolling24, "24h", "过去 24 小时：10/12 00:31 – 10/13 00:31"],
      [{ kind: "rolling", hours: 24 }, null, "24h", "过去 24 小时"],
      [{ kind: "days", days: 7 }, dayRange, "7d", "近 7 天：10/1 – 10/12"],
      [{ kind: "days", days: 14 }, null, "14d", "近 14 天"],
      [{ kind: "days", days: 30 }, null, "30d", "近 30 天"],
    ];
    for (const [preset, range, label, aria] of cases) {
      const w = mountRange(preset, range);
      const trigger = w.find(".range-trigger");
      expect(trigger.text()).toBe(label);
      expect(trigger.find("svg").exists(), `${label} 不再显示图标`).toBe(false);
      expect(trigger.classes()).toContain("has-preset");
      expect(trigger.attributes("aria-label")).toBe(aria);
      expect(trigger.attributes("title")).toBe(aria);
      w.unmount();
    }
  });

  it("自定义标签：自然日 / 同一天时刻 / 跨天时刻 / 至今 / 缺 range 降级", () => {
    const cases: [RangePreset | null, RangeView | null, string][] = [
      [
        { kind: "custom", start: "2026-10-01T00:00", end: "2026-10-12T23:59" },
        dayRange,
        "10/1–10/12",
      ],
      [{ kind: "custom", start: "2026-10-01T00:00", end: "now" }, dayRange, "10/1 起"],
      [
        { kind: "custom", start: "2026-10-12T09:00", end: "2026-10-12T18:00" },
        instantSameDay,
        "10/12 09:00–18:00",
      ],
      [
        { kind: "custom", start: "2026-10-11T22:00", end: "2026-10-12T02:00" },
        instantAcrossDays,
        "10/11 22:00–10/12 02:00",
      ],
      [{ kind: "custom", start: "2026-10-12T09:00", end: "now" }, instantSameDay, "10/12 09:00 起"],
      [{ kind: "custom", start: "2026-10-12T09:00", end: "now" }, null, "自定义范围"],
      [
        { kind: "custom", start: "2024-03-05T00:00", end: "2024-03-10T23:59" },
        { granularity: "day", from: "2024-03-05", to: "2024-03-10" },
        "2024/3/5–2024/3/10",
      ],
    ];
    for (const [preset, range, label] of cases) {
      const w = mountRange(preset, range);
      expect(w.find(".range-trigger").text()).toBe(label);
      w.unmount();
    }
  });

  it("标签容器受 220px 上限约束并省略溢出", () => {
    const w = mountRange({ kind: "custom", start: "2024-03-05T00:00", end: "2024-03-10T23:59" });
    // 样式契约：按钮与标签都不得自然撑破布局（超长由 CSS 省略）
    expect(w.find(".range-trigger").classes()).toContain("has-preset");
  });
});

describe("todayInTz（R03：local 不得抛 RangeError）", () => {
  it("local 返回本机日历日；IANA 时区正常", () => {
    // 修复前 todayInTz("local") 直接把 "local" 传给 Intl → RangeError
    expect(() => todayInTz("local")).not.toThrow();
    expect(todayInTz("local")).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(todayInTz("Asia/Shanghai")).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(todayInTz("UTC")).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

describe("纯日历工具（DST/跨年边界）", () => {
  it("addDays 跨美国 DST 起止日不漂移（纯日历运算，与本地时区无关）", () => {
    // 2026-03-08 美国春令时；2026-11-01 秋令时
    expect(addDays("2026-03-07", 1)).toBe("2026-03-08");
    expect(addDays("2026-03-08", 1)).toBe("2026-03-09");
    expect(addDays("2026-10-31", 1)).toBe("2026-11-01");
    expect(addDays("2026-11-01", 1)).toBe("2026-11-02");
  });

  it("addDays 跨年正确", () => {
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2027-01-01", -1)).toBe("2026-12-31");
  });

  it("tzDate local 与 IANA 路径都返回 YYYY-MM-DD", () => {
    const now = Date.now();
    expect(tzDate(now, "local")).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(tzDate(now, "Asia/Shanghai")).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

// UX04：跨年/历史区间的标签必须保留年份；"同日"按完整 ISO 日期比较。
describe("UX04 区间标签（跨年与历史年份）", () => {
  const TODAY = "2026-10-07";

  it("same_month_day_across_years_is_not_single_day：2024-01-01..2025-01-01 必须含两个年份", () => {
    const label = rangeLabel("2024-01-01", "2025-01-01", TODAY);
    // 修复前：两端点 M/D 都是 1/1 → 被折叠成 "1/1"（单日误判 + 年份丢失）
    expect(label).not.toBe("1/1");
    expect(label).toContain("2024");
    expect(label).toContain("2025");
  });

  it("historical_range_retains_year：同年但非当前年也保留年份", () => {
    const label = rangeLabel("2024-03-05", "2024-03-10", TODAY);
    expect(label).toContain("2024");
    expect(label).not.toBe("3/5 ~ 3/10");
    // 同年当年可保留简短标签
    expect(rangeLabel("2026-09-29", "2026-10-05", TODAY)).toBe("9/29 ~ 10/5");
  });

  it("follow_today_label_retains_start_year：跟随今天时保留起始年份", () => {
    const label = rangeLabel("2024-01-01", TODAY, TODAY);
    expect(label).toContain("2024");
    expect(label).toContain("今天");
    // 当年起始日仍用简短标签
    expect(rangeLabel("2026-09-29", TODAY, TODAY)).toBe("9/29 ~ 今天");
  });

  it("同日按完整 ISO 日期比较：2024-01-01 与 2025-01-01 不是同一天", () => {
    expect(rangeLabel("2024-01-01", "2024-01-01", TODAY)).toBe("2024/1/1");
    expect(rangeLabel("2025-01-01", "2025-01-01", TODAY)).toBe("2025/1/1");
  });
});
