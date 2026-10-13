// U01（UI 调整计划 2026-10-13）：时间预设的纯函数契约。
// 前端只发预设参数、不计算查询边界；按钮标签由 preset 直接可算的部分
// （当日/7d/24h）与后端回传 range 决定的形态（自定义）拼装。
import { describe, expect, it, vi } from "vitest";

// 固定"今天"，让标签的年份规则可预测（真实时钟与本机时区无关）。
vi.mock("./dates", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./dates")>();
  return { ...actual, todayInTz: () => "2026-10-12" };
});

import {
  normalizePreset,
  presetAriaLabel,
  presetKey,
  presetLabel,
  presetToQueryParams,
  type RangePreset,
} from "./rangePreset";
import type { RangeView } from "../types";

const TZ = "Asia/Shanghai";

const NONE = {
  days: null,
  rollingHours: null,
  startAt: null,
  endAt: null,
  untilNow: null,
};

const dayRange: RangeView = {
  granularity: "day",
  from: "2026-10-01",
  to: "2026-10-12",
};

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

const customDay: RangePreset = {
  kind: "custom",
  start: "2026-10-01T00:00",
  end: "2026-10-12T23:59",
};
const customUntilNowDay: RangePreset = {
  kind: "custom",
  start: "2026-10-01T00:00",
  end: "now",
};
const customInstant: RangePreset = {
  kind: "custom",
  start: "2026-10-12T09:00",
  end: "2026-10-12T18:00",
};
const customInstantUntilNow: RangePreset = {
  kind: "custom",
  start: "2026-10-12T09:00",
  end: "now",
};

describe("presetToQueryParams（只发预设参数）", () => {
  it("null 与 all 都不带任何时间参数", () => {
    expect(presetToQueryParams(null)).toEqual(NONE);
    expect(presetToQueryParams({ kind: "all" })).toEqual(NONE);
  });

  it("自然日预设只发 days", () => {
    expect(presetToQueryParams({ kind: "days", days: 1 })).toEqual({ ...NONE, days: 1 });
    expect(presetToQueryParams({ kind: "days", days: 7 })).toEqual({ ...NONE, days: 7 });
    expect(presetToQueryParams({ kind: "days", days: 14 })).toEqual({ ...NONE, days: 14 });
    expect(presetToQueryParams({ kind: "days", days: 30 })).toEqual({ ...NONE, days: 30 });
  });

  it("24h 只发 rollingHours", () => {
    expect(presetToQueryParams({ kind: "rolling", hours: 24 })).toEqual({
      ...NONE,
      rollingHours: 24,
    });
  });

  it("自定义发 startAt + endAt；至今发 untilNow 而不是 endAt", () => {
    expect(presetToQueryParams(customInstant)).toEqual({
      ...NONE,
      startAt: "2026-10-12T09:00",
      endAt: "2026-10-12T18:00",
    });
    expect(presetToQueryParams(customInstantUntilNow)).toEqual({
      ...NONE,
      startAt: "2026-10-12T09:00",
      untilNow: true,
    });
  });
});

describe("presetKey / normalizePreset（快照身份与非法值回退）", () => {
  it("presetKey 对 null 与 all 等价", () => {
    expect(presetKey(null)).toBe("all");
    expect(presetKey({ kind: "all" })).toBe("all");
    expect(presetKey({ kind: "days", days: 7 })).toBe("days:7");
    expect(presetKey({ kind: "rolling", hours: 24 })).toBe("rolling:24");
    expect(presetKey(customUntilNowDay)).toBe("custom:2026-10-01T00:00:now");
  });

  it("normalizePreset 接受合法形态", () => {
    expect(normalizePreset({ kind: "all" })).toEqual({ kind: "all" });
    expect(normalizePreset({ kind: "days", days: 30 })).toEqual({ kind: "days", days: 30 });
    expect(normalizePreset({ kind: "rolling", hours: 24 })).toEqual({ kind: "rolling", hours: 24 });
    expect(normalizePreset(customInstant)).toEqual(customInstant);
    expect(normalizePreset(customInstantUntilNow)).toEqual(customInstantUntilNow);
  });

  it("normalizePreset 把缺失/非法值一律回退为 null（全部时间）", () => {
    expect(normalizePreset(undefined)).toBeNull();
    expect(normalizePreset(null)).toBeNull();
    expect(normalizePreset("全部")).toBeNull();
    expect(normalizePreset({ kind: "days", days: 3 })).toBeNull();
    expect(normalizePreset({ kind: "rolling", hours: 12 })).toBeNull();
    expect(normalizePreset({ kind: "custom", start: "2026-10-01", end: "now" })).toBeNull();
    expect(normalizePreset({ kind: "custom", start: "2026-10-01T00:00" })).toBeNull();
    expect(normalizePreset({ kind: "custom", start: "2026-10-01T00:00", end: 42 })).toBeNull();
  });
});

describe("presetLabel（按钮标签）", () => {
  it("全部时间与自然日/滚动预设由 preset 直接可算", () => {
    expect(presetLabel(null, null, TZ)).toBe("全部时间");
    expect(presetLabel({ kind: "all" }, null, TZ)).toBe("全部时间");
    expect(presetLabel({ kind: "days", days: 1 }, null, TZ)).toBe("当日");
    expect(presetLabel({ kind: "days", days: 7 }, null, TZ)).toBe("7d");
    expect(presetLabel({ kind: "days", days: 14 }, null, TZ)).toBe("14d");
    expect(presetLabel({ kind: "days", days: 30 }, null, TZ)).toBe("30d");
    expect(presetLabel({ kind: "rolling", hours: 24 }, null, TZ)).toBe("24h");
  });

  it("自定义缺 range 时降级为「自定义范围」", () => {
    expect(presetLabel(customDay, null, TZ)).toBe("自定义范围");
    expect(presetLabel(customInstant, undefined, TZ)).toBe("自定义范围");
  });

  it("自定义自然日模式：区间与至今", () => {
    expect(presetLabel(customDay, dayRange, TZ)).toBe("10/1–10/12");
    expect(presetLabel(customUntilNowDay, dayRange, TZ)).toBe("10/1 起");
    expect(
      presetLabel(customDay, { granularity: "day", from: "2026-10-05", to: "2026-10-05" }, TZ),
    ).toBe("10/5");
  });

  it("自定义时刻模式：同一天、跨天、至今", () => {
    expect(presetLabel(customInstant, instantSameDay, TZ)).toBe("10/12 09:00–18:00");
    expect(
      presetLabel({ ...customInstant, start: "2026-10-11T22:00" }, instantAcrossDays, TZ),
    ).toBe("10/11 22:00–10/12 02:00");
    expect(presetLabel(customInstantUntilNow, instantSameDay, TZ)).toBe("10/12 09:00 起");
  });

  it("非当前年或跨年带年份（沿用 rangeLabel 规则）", () => {
    expect(
      presetLabel(customDay, { granularity: "day", from: "2024-03-05", to: "2024-03-10" }, TZ),
    ).toBe("2024/3/5–2024/3/10");
    expect(
      presetLabel(customDay, { granularity: "day", from: "2025-12-30", to: "2026-01-02" }, TZ),
    ).toBe("2025/12/30–2026/1/2");
    expect(
      presetLabel(
        customInstant,
        { ...instantSameDay, start_local: "2025-12-31T22:00", end_local: "2026-01-01T02:00" },
        TZ,
      ),
    ).toBe("2025/12/31 22:00–2026/1/1 02:00");
  });
});

describe("presetAriaLabel（完整范围的可访问名称）", () => {
  it("缺 range 时只写预设名", () => {
    expect(presetAriaLabel(null, null, TZ)).toBe("全部时间");
    expect(presetAriaLabel({ kind: "days", days: 7 }, null, TZ)).toBe("近 7 天");
    expect(presetAriaLabel({ kind: "days", days: 1 }, null, TZ)).toBe("当日");
    expect(presetAriaLabel({ kind: "rolling", hours: 24 }, null, TZ)).toBe("过去 24 小时");
    expect(presetAriaLabel(customInstant, null, TZ)).toBe("自定义范围");
  });

  it("有 range 时写完整范围", () => {
    expect(presetAriaLabel({ kind: "days", days: 7 }, dayRange, TZ)).toBe("近 7 天：10/1 – 10/12");
    expect(
      presetAriaLabel(
        { kind: "days", days: 1 },
        { granularity: "day", from: "2026-10-12", to: "2026-10-12" },
        TZ,
      ),
    ).toBe("当日：10/12");
    expect(presetAriaLabel({ kind: "rolling", hours: 24 }, rolling24, TZ)).toBe(
      "过去 24 小时：10/12 00:31 – 10/13 00:31",
    );
    expect(presetAriaLabel(customInstant, instantAcrossDays, TZ)).toBe(
      "自定义范围：10/11 22:00 – 10/12 02:00",
    );
    expect(presetAriaLabel(customUntilNowDay, dayRange, TZ)).toBe("自定义范围：10/1 起");
  });

  it("非当前年带年份", () => {
    expect(
      presetAriaLabel(customDay, { granularity: "day", from: "2024-03-05", to: "2024-03-10" }, TZ),
    ).toBe("自定义范围：2024/3/5 – 2024/3/10");
  });
});
