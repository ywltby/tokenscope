import { afterEach, describe, expect, it } from "vitest";
import { formatCompactTokenAxis, formatSummaryTokenUnit, toSuperscript } from "./formatTokens";
import { applyLocale, DEFAULT_LOCALE } from "./translate";

afterEach(() => {
  applyLocale(DEFAULT_LOCALE);
});

describe("toSuperscript（上标指数）", () => {
  it("转换正负整数指数为 Unicode 上标", () => {
    expect(toSuperscript(15)).toBe("¹⁵");
    expect(toSuperscript(16)).toBe("¹⁶");
    expect(toSuperscript(29)).toBe("²⁹");
    expect(toSuperscript(100)).toBe("¹⁰⁰");
    expect(toSuperscript(-3)).toBe("⁻³");
  });
});

describe("formatSummaryTokenUnit（总 token 换算单位）", () => {
  it("中文：小于 1 万标 tokens，按 万 / 亿 / 万亿 进位，万亿再进位（≥ 10¹⁶）转上标科学计数法", () => {
    applyLocale("zh-CN");
    expect(formatSummaryTokenUnit(0)).toBe("tokens");
    expect(formatSummaryTokenUnit(1)).toBe("tokens");
    expect(formatSummaryTokenUnit(850)).toBe("tokens");
    expect(formatSummaryTokenUnit(9_999)).toBe("tokens");

    expect(formatSummaryTokenUnit(10_000)).toBe("≈ 1.00 万 tokens");
    expect(formatSummaryTokenUnit(69_000)).toBe("≈ 6.90 万 tokens");
    expect(formatSummaryTokenUnit(1_234_567)).toBe("≈ 123.46 万 tokens");
    expect(formatSummaryTokenUnit(99_999_949)).toBe("≈ 9999.99 万 tokens");

    // 两位小数进位达到 10000.00 万 → 1.00 亿
    expect(formatSummaryTokenUnit(99_999_950)).toBe("≈ 1.00 亿 tokens");
    expect(formatSummaryTokenUnit(99_999_999)).toBe("≈ 1.00 亿 tokens");
    expect(formatSummaryTokenUnit(100_000_000)).toBe("≈ 1.00 亿 tokens");
    expect(formatSummaryTokenUnit(13_862_000_000)).toBe("≈ 138.62 亿 tokens");
    expect(formatSummaryTokenUnit(13_862_345_678)).toBe("≈ 138.62 亿 tokens");
    expect(formatSummaryTokenUnit(999_999_499_999)).toBe("≈ 9999.99 亿 tokens");

    // 10¹² 起用万亿
    expect(formatSummaryTokenUnit(999_999_500_000)).toBe("≈ 1.00 万亿 tokens");
    expect(formatSummaryTokenUnit(1_000_000_000_000)).toBe("≈ 1.00 万亿 tokens");
    expect(formatSummaryTokenUnit(13_862_000_000_000)).toBe("≈ 13.86 万亿 tokens");
    expect(formatSummaryTokenUnit("9999994999999999")).toBe("≈ 9999.99 万亿 tokens");

    // 万亿往上进位（≥ 10¹⁶ 或四舍五入触及 10000.00 万亿）→ 上标科学计数法
    expect(formatSummaryTokenUnit("9999995000000000")).toBe("≈ 1.00 × 10¹⁶ tokens");
    expect(formatSummaryTokenUnit("10000000000000000")).toBe("≈ 1.00 × 10¹⁶ tokens");
    expect(formatSummaryTokenUnit("12345678901234567")).toBe("≈ 1.23 × 10¹⁶ tokens");
    expect(formatSummaryTokenUnit("99995000000000000")).toBe("≈ 1.00 × 10¹⁷ tokens");
    expect(formatSummaryTokenUnit(123456789012345678901234567890n)).toBe("≈ 1.23 × 10²⁹ tokens");
  });

  it("英文：小于 1K 标 tokens，按 K / M / B / T 进位，T 再进位（≥ 10¹⁵）转上标科学计数法", () => {
    applyLocale("en-US");
    expect(formatSummaryTokenUnit(0)).toBe("tokens");
    expect(formatSummaryTokenUnit(500)).toBe("tokens");
    expect(formatSummaryTokenUnit(999)).toBe("tokens");

    expect(formatSummaryTokenUnit(1_000)).toBe("≈ 1.00K tokens");
    expect(formatSummaryTokenUnit(69_000)).toBe("≈ 69.00K tokens");
    expect(formatSummaryTokenUnit(999_994)).toBe("≈ 999.99K tokens");
    expect(formatSummaryTokenUnit(999_995)).toBe("≈ 1.00M tokens");

    expect(formatSummaryTokenUnit(1_000_000)).toBe("≈ 1.00M tokens");
    expect(formatSummaryTokenUnit(12_340_000)).toBe("≈ 12.34M tokens");
    expect(formatSummaryTokenUnit(999_994_999)).toBe("≈ 999.99M tokens");
    expect(formatSummaryTokenUnit(999_995_000)).toBe("≈ 1.00B tokens");

    expect(formatSummaryTokenUnit(1_000_000_000)).toBe("≈ 1.00B tokens");
    expect(formatSummaryTokenUnit(13_862_000_000)).toBe("≈ 13.86B tokens");
    expect(formatSummaryTokenUnit(999_994_999_999)).toBe("≈ 999.99B tokens");

    // 10¹² 起用 T
    expect(formatSummaryTokenUnit(999_995_000_000)).toBe("≈ 1.00T tokens");
    expect(formatSummaryTokenUnit(1_000_000_000_000)).toBe("≈ 1.00T tokens");
    expect(formatSummaryTokenUnit(13_862_000_000_000)).toBe("≈ 13.86T tokens");
    expect(formatSummaryTokenUnit("999994999999999")).toBe("≈ 999.99T tokens");

    // T 往上进位（≥ 10¹⁵ 或四舍五入触及 1000.00T）→ 上标科学计数法
    expect(formatSummaryTokenUnit("999995000000000")).toBe("≈ 1.00 × 10¹⁵ tokens");
    expect(formatSummaryTokenUnit("1000000000000000")).toBe("≈ 1.00 × 10¹⁵ tokens");
    expect(formatSummaryTokenUnit("12345678901234567")).toBe("≈ 1.23 × 10¹⁶ tokens");
    expect(formatSummaryTokenUnit(123456789012345678901234567890n)).toBe("≈ 1.23 × 10²⁹ tokens");
    expect(formatSummaryTokenUnit(13_862_000_000)).not.toContain("10k");
  });

  it("非有限值或非法字符串回退为纯单位 tokens", () => {
    expect(formatSummaryTokenUnit(Number.NaN)).toBe("tokens");
    expect(formatSummaryTokenUnit(Number.POSITIVE_INFINITY)).toBe("tokens");
    expect(formatSummaryTokenUnit("not-a-number")).toBe("tokens");
  });
});

describe("formatCompactTokenAxis（图表坐标轴刻度）", () => {
  it("中文刻度支持 万 / 亿 / 万亿 与 ≥ 10¹⁶ 上标科学计数法", () => {
    applyLocale("zh-CN");
    expect(formatCompactTokenAxis(0)).toBe("0");
    expect(formatCompactTokenAxis(500)).toBe("500");
    expect(formatCompactTokenAxis(50_000)).toBe("5万");
    expect(formatCompactTokenAxis(150_000_000)).toBe("1.5亿");
    expect(formatCompactTokenAxis(1_000_000_000_000)).toBe("1万亿");
    expect(formatCompactTokenAxis(2_500_000_000_000)).toBe("2.5万亿");
    expect(formatCompactTokenAxis(1e16)).toBe("1 × 10¹⁶");
    expect(formatCompactTokenAxis(1.24e16)).toBe("1.2 × 10¹⁶");
    expect(formatCompactTokenAxis(9.99e16)).toBe("1 × 10¹⁷");
    expect(formatCompactTokenAxis(1.5e290, 20)).toBe("1.5 × 10³¹⁰");
  });

  it("英文刻度支持 K / M / B / T 与 ≥ 10¹⁵ 上标科学计数法", () => {
    applyLocale("en-US");
    expect(formatCompactTokenAxis(500)).toBe("500");
    expect(formatCompactTokenAxis(50_000)).toBe("50K");
    expect(formatCompactTokenAxis(1_500_000)).toBe("1.5M");
    expect(formatCompactTokenAxis(2_000_000_000)).toBe("2B");
    expect(formatCompactTokenAxis(1_500_000_000_000)).toBe("1.5T");
    expect(formatCompactTokenAxis(1e15)).toBe("1 × 10¹⁵");
    expect(formatCompactTokenAxis(1.24e15)).toBe("1.2 × 10¹⁵");
    expect(formatCompactTokenAxis(9.99e15)).toBe("1 × 10¹⁶");
    expect(formatCompactTokenAxis(1.5e290, 20)).toBe("1.5 × 10³¹⁰");
  });

  it("scalePow10 > 0 下缩放查看常规量级刻度时还原走单位阶梯且不溢出 10^scalePow10", () => {
    applyLocale("zh-CN");
    expect(formatCompactTokenAxis(2e-20, 21)).toBe("20");
    expect(formatCompactTokenAxis(5e-17, 21)).toBe("5万");
    expect(formatCompactTokenAxis(2e-305, 310)).toBe("20万");

    applyLocale("en-US");
    expect(formatCompactTokenAxis(2e-20, 21)).toBe("20");
    expect(formatCompactTokenAxis(2e-305, 310)).toBe("200K");
  });
});
