// UX07：金额格式化与 token 显示词契约。
// - request_amount_matches_breakdown_result：请求金额与公式结果同一规则
//  （0.0126 → $0.0126 四位）；
// - tiny_nonzero_is_never_displayed_as_zero：1e-8 不得显示为 $0.00/$0；
// - zero_and_unknown_are_distinct：0 = $0.00，未知由调用点显式传递；
// - unit_price_keeps_its_precision_and_unit：单价场景保留有效精度；
// - all_token_views_share_labels_and_order：表头/图例/摘要/公式消费
//   同一份 token 元数据（缓存命中显示词统一，技术键 cache_read 保留）。
import { describe, expect, it } from "vitest";
import { formatMoney } from "./formatMoney";
import { TOKEN_BUCKETS, tokenBucketLabel } from "./tokenDisplay";
import { chartTokens } from "../styles/chartTheme";
import { SERIES } from "./chartData";
import { KIND_LABEL } from "./costBreakdown";

describe("formatMoney（UX07 单一金额入口）", () => {
  it("request_amount_matches_breakdown_result：0.0126 四位精度", () => {
    expect(formatMoney(0.0126, "request")).toBe("$0.0126");
    expect(formatMoney(0.0126, "unit")).toBe("$0.0126");
  });

  it("tiny_nonzero_is_never_displayed_as_zero：1e-8 用科学记数法表达非零", () => {
    for (const scenario of ["request", "summary", "unit"] as const) {
      const s = formatMoney(1e-8, scenario);
      expect(s, scenario).not.toBe("$0.00");
      expect(s, scenario).not.toBe("$0");
      expect(s, scenario).not.toContain("0.000000");
      expect(s, scenario).toMatch(/e-8$/);
    }
  });

  it("zero_and_unknown_are_distinct：0 = $0.00；未知不是 0", () => {
    expect(formatMoney(0, "request")).toBe("$0.00");
    expect(formatMoney(0, "summary")).toBe("$0.00");
    expect(formatMoney(null)).toBe("未知");
    expect(formatMoney(undefined)).toBe("未知");
    expect(formatMoney(Number.NaN)).toBe("未知");
  });

  it("unit_price_keeps_its_precision_and_unit：单价保留有效精度，金额保持习惯精度", () => {
    // RC07：单价 = JS number 的最短可往返十进制有效表示，不强制舍入。
    expect(formatMoney(0.5, "unit")).toBe("$0.5");
    expect(formatMoney(0.0000005, "unit")).toBe("$5e-7");
    expect(formatMoney(0.000005, "unit")).toBe("$0.000005");
    expect(formatMoney(3, "unit")).toBe("$3");
    // request/summary 保持既有习惯精度（不受 unit 改动影响）。
    expect(formatMoney(12.5, "summary")).toBe("$12.50");
    expect(formatMoney(0.0126, "request")).toBe("$0.0126");
    expect(formatMoney(2.5, "request")).toBe("$2.50");
    // 返回完整 USD 文本（含 $），调用方不得重复拼接。
    expect(formatMoney(1.25).startsWith("$")).toBe(true);
  });

  // RC07：复核现象——formatMoney(1.234567, "unit") 曾返回 $1.23（scenario
  // 被丢弃 + ≥1 固定两位），真实单价精度丢失。
  it("unit_rates_keep_significant_precision：单价格式保留有效精度与单位", () => {
    const cases: [number, string][] = [
      [1.234567, "$1.234567"],
      [12.3456789, "$12.3456789"],
      [0.123456789, "$0.123456789"],
      [1e-8, "$1e-8"],
      [0, "$0.00"],
      [2.5, "$2.5"],
      [0.0000012, "$0.0000012"],
    ];
    for (const [v, expected] of cases) {
      expect(formatMoney(v, "unit"), `unit ${v}`).toBe(expected);
    }
    // 未知与非法值不得显示成合法价格
    expect(formatMoney(null, "unit")).toBe("未知");
    expect(formatMoney(undefined, "unit")).toBe("未知");
    expect(formatMoney(Number.NaN, "unit")).toBe("未知");
    expect(formatMoney(Number.POSITIVE_INFINITY, "unit")).toBe("未知");
    expect(formatMoney(Number.NEGATIVE_INFINITY, "unit")).toBe("未知");
    // 1e-8 明确非零（不得是 $0.00/$0）
    const tiny = formatMoney(1e-8, "unit");
    expect(tiny).not.toBe("$0.00");
    expect(tiny).not.toBe("$0");
    // 美元符号只出现一次
    for (const [, text] of cases) {
      expect(text.split("$").length - 1, text).toBe(1);
    }
  });
});

describe("all_token_views_share_labels_and_order（UX07 token 显示词同源）", () => {
  it("四桶 key 顺序：输入、输出、缓存写、缓存命中（技术键 cache_read 保留）", () => {
    expect(TOKEN_BUCKETS.map((b) => b.key)).toEqual([
      "input",
      "output",
      "cache_write",
      "cache_read",
    ]);
    expect(tokenBucketLabel("cache_read")).toBe("缓存命中");
  });

  it("图表图例/摘要与费用公式消费同一元数据", () => {
    const legend = chartTokens("light").series;
    expect(legend.map((s) => s.key)).toEqual(TOKEN_BUCKETS.map((b) => b.key));
    expect(legend.map((s) => s.label)).toEqual(TOKEN_BUCKETS.map((b) => b.label));
    // 摘要（chartData.SERIES）
    expect(SERIES.map((s) => s.name)).toEqual(TOKEN_BUCKETS.map((b) => b.key));
    expect(SERIES.map((s) => s.label)).toEqual(TOKEN_BUCKETS.map((b) => b.label));
    // 费用公式行（costBreakdown.KIND_LABEL）
    expect(TOKEN_BUCKETS.map((b) => KIND_LABEL[b.key])).toEqual(
      TOKEN_BUCKETS.map((b) => (b.key === "input" ? "输入（扣除缓存）" : b.label)),
    );
  });
});
