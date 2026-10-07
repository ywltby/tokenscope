// SF07：三态单价展示契约——null = 未知、"same_as_input" = 同输入价、
// 0 = $0（显式免费）；分段/峰谷嵌套视图保留三态 RateSpec，缺失覆盖值
// 不得展示成免费。
import { describe, expect, it } from "vitest";
import { fmtPriceOrUnknown, formatRates, formatTieredPricing } from "./tieredPrice";
import type { PricingEntry, RateSpecView } from "../types";

describe("fmtPriceOrUnknown（三态单价展示）", () => {
  it("unknown_zero_same_as_input_displayed_distinctly：三态互不混淆", () => {
    expect(fmtPriceOrUnknown(null)).toBe("未知");
    expect(fmtPriceOrUnknown(undefined)).toBe("未知");
    expect(fmtPriceOrUnknown("same_as_input")).toBe("同输入价");
    expect(fmtPriceOrUnknown(0)).toBe("$0");
    expect(fmtPriceOrUnknown(1.5)).toBe("$1.50");
    expect(fmtPriceOrUnknown(0.5)).toBe("$0.5000");
    expect(fmtPriceOrUnknown(0.0000012)).toBe("$0.000001");
  });

  it("rates_line_keeps_three_states：四类单价行携带 same_as_input", () => {
    const rates = {
      input: 8,
      output: 30,
      cache_write: 0,
      cache_read: "same_as_input" as RateSpecView,
    };
    expect(formatRates(rates)).toBe("输入 $8.00 · 输出 $30.00 · 缓存写 $0 · 缓存读 同输入价");
  });

  it("tiered_views_keep_rate_specs：嵌套分段/峰谷视图不把缺失标成免费", () => {
    const e = {
      prefix: "seg",
      input: 1,
      output: 1,
      cache_write: null,
      cache_read: "same_as_input",
      source: "外置",
      base_incomplete: false,
      has_tiered_pricing: true,
      basis: "prompt_tokens",
      segments: [
        {
          label: "大请求",
          min_tokens: 1000000,
          max_tokens: null,
          prices: { input: 3, output: 3, cache_write: null, cache_read: null },
        },
      ],
      schedules: [
        {
          label: "峰谷",
          timezone: "UTC",
          prices: { input: 0.5, output: 0.5, cache_write: null, cache_read: null },
          periods: [
            {
              start_time: "00:00",
              end_time: "08:00",
              prices: { input: 0.2, output: 0.2, cache_write: null, cache_read: null },
            },
          ],
        },
      ],
    } as unknown as PricingEntry;
    const lines = formatTieredPricing(e);
    expect(
      lines.some((l) => l.includes("大请求") && l.includes("缓存读 未知")),
      `分段缺失分项必须显示"未知"（不是 $0）: ${lines.join(" / ")}`,
    ).toBe(true);
    expect(
      lines.some((l) => l.includes("峰谷") && l.includes("缓存写 未知")),
      `峰谷缺失分项必须显示"未知": ${lines.join(" / ")}`,
    ).toBe(true);
  });
});
