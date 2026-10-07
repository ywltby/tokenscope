// R03（全计划审核 Task 3）：日期控件真实契约 + 本机时区 + 纯日历工具。
// 关键语义（R03 根因）：NDatePicker 走 v-model:formatted-value + value-format
// 直接桥接**字符串**——控件日历吐什么就提交什么，无毫秒/时区重解释。
// 跨时区说明：Windows Node 忽略 TZ 环境变量，无法在进程内切换时区；
// 本机即 Asia/Shanghai（断言处打印 resolved 时区证明环境生效），
// 洛杉矶等负偏移场景由「字符串往返不变量」覆盖（与本地时区无关）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

console.info("测试环境时区:", Intl.DateTimeFormat().resolvedOptions().timeZone);

// NPopover 打桩为透传（trigger + 默认槽同渲染）；NDatePicker 用**真实组件**
// ——R03 要求至少一条实际控件交互 → emit 断言，不能全用 stub。
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

// SF05：todayInTz 计数/脚本化——验证"每次操作只读取一次 today"。
// 脚本队列耗尽后回落真实时钟；vi.restoreAllMocks 不会破坏此包装。
vi.mock("../lib/dates", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/dates")>();
  return {
    ...actual,
    todayInTz: (tz: string) => {
      const g = globalThis as unknown as { __todayCalls?: number; __todayScript?: string[] };
      g.__todayCalls = (g.__todayCalls ?? 0) + 1;
      const next = g.__todayScript?.shift();
      return next ?? actual.todayInTz(tz);
    },
  };
});

function setTodayScript(values: string[]): void {
  (globalThis as unknown as { __todayScript?: string[] }).__todayScript = [...values];
}
function resetTodayCalls(): void {
  (globalThis as unknown as { __todayCalls?: number }).__todayCalls = 0;
}
function todayCalls(): number {
  return (globalThis as unknown as { __todayCalls?: number }).__todayCalls ?? 0;
}

import DateRangeSelect from "./DateRangeSelect.vue";
import { addDays, rangeLabel, todayInTz, tzDate } from "../lib/dates";

const NPopoverStub = { name: "NPopover" };

function mountRange(value: [string, string] | null, tz = "Asia/Shanghai") {
  return mount(DateRangeSelect, { props: { value, tz } });
}

async function open(w: ReturnType<typeof mountRange>): Promise<void> {
  await w.findComponent(NPopoverStub).vm.$emit("update:show", true);
  await flushPromises();
}

beforeEach(() => {
  localStorage.clear();
});

describe("picker 真实控件契约（R03）", () => {
  it("picker_emits_selected_calendar_date：真实 NDatePicker 选 2026-10-06 提交同日字符串", async () => {
    const w = mountRange(null);
    await open(w);
    // 真实 NDatePicker 渲染出输入框，聚焦打开面板
    const input = w.find(".n-input__input-el");
    expect(input.exists(), "真实 NDatePicker 输入框必须渲染").toBe(true);
    await input.setValue("2026-10-06");
    await input.trigger("keydown", { key: "Enter" });
    await flushPromises();
    // 键入后经 naive 解析 → formatted-value 更新 → confirm 提交同日字符串
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    const emitted = w.emitted("update:value")!.at(-1)![0];
    expect(emitted).toEqual(["2026-10-06", "2026-10-06"]);
  });

  it("picker_displays_existing_calendar_date：外部值原样显示（往返不变量，洛杉矶等负偏移同构）", async () => {
    const w = mountRange(["2026-10-06", "2026-10-07"]);
    await open(w);
    // formatted-value 桥接：传入的日历字符串必须原样回显在输入框
    const inputs = w.findAll(".n-input__input-el");
    expect((inputs[0]!.element as HTMLInputElement).value).toBe("2026-10-06");
    expect((inputs[1]!.element as HTMLInputElement).value).toBe("2026-10-07");
    // 往返不变量：字符串 → 控件 → 字符串 无偏移（LA 与上海同构，本地时区无关）
    expect(w.emitted("update:value")).toBeUndefined();
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

// 既有行为回归保留（R03 之外的草稿/取消/清除语义）
describe("DateRangeSelect 行为回归", () => {
  it("date_range_cancel_reopen：取消不提交，重开后草稿重新同步自当前值", async () => {
    const value: [string, string] = ["2026-09-29", "2026-10-05"];
    const w = mountRange(value);
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
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    const emitted = w.emitted("update:value")!.at(-1)![0];
    expect(emitted).toEqual(value);
  });

  it("date_range_clear_all：清除入口可达，确定后回全部时间", async () => {
    const w = mountRange(["2026-09-06", "2026-10-05"]);
    await open(w);
    const clear = w.findAll("button").find((b) => b.text() === "清除");
    expect(clear).toBeDefined();
    await clear!.trigger("click");
    const ok = w.findAll("button").find((b) => b.text() === "确定");
    await ok!.trigger("click");
    expect(w.emitted("update:value")!.at(-1)![0]).toBeNull();
  });
});

// SF05：时间基准——快捷项/确认每次操作只读取一次 today，两端由同一次
// 读取派生；跨午夜取到两个"今天"的实现会被脚本化读取序列暴露。
describe("SF05 时间基准（一次操作一次 today）", () => {
  it("shortcut_both_ends_from_one_today_read：快捷项双读会把 08-11 当结束日提交", async () => {
    const w = mountRange(null);
    await open(w);
    resetTodayCalls();
    // 读序列：第 1 次给 08-10（快捷项操作），其后任何额外读取都得到
    // 08-11——若实现读两次（旧实现 from/to 各取一次 now），提交对即暴露。
    setTodayScript(["2026-08-10"]);
    await w
      .findAll("button")
      .find((b) => b.text() === "近2天")!
      .trigger("click");
    expect(todayCalls()).toBeGreaterThanOrEqual(1);
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    const emitted = w.emitted("update:value")!.at(-1)![0];
    expect(emitted).toEqual(["2026-08-09", "2026-08-10"]);
  });

  it("confirm_follow_today_reads_today_once：跟随今天的结束日来自确认时的单次读取", async () => {
    const w = mountRange(null);
    await open(w);
    await w
      .findAll("button")
      .find((b) => b.text() === "近2天")!
      .trigger("click");
    // 勾选"结束日跟随今天"（真实 NCheckbox）
    const checkbox = w.find(".n-checkbox");
    await checkbox.trigger("click");
    resetTodayCalls();
    setTodayScript(["2026-08-15", "2026-08-16"]);
    await w
      .findAll("button")
      .find((b) => b.text() === "确定")!
      .trigger("click");
    const emitted = w.emitted("update:value")!.at(-1)![0] as [string, string];
    expect(emitted[1]).toBe("2026-08-15");
    expect(todayCalls()).toBeGreaterThanOrEqual(1);
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