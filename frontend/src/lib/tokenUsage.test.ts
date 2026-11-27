import { describe, expect, it } from "vitest";
import { fmtNum } from "../types";
import {
  bigIntToScaledNumber,
  displayTokens,
  percentageFixed2BigInt,
  ratioPercentBigInt,
  totalTokens,
} from "./tokenUsage";

describe("总输入展示（原生 BigInt 精确累计）", () => {
  it("Codex 恢复日志总输入，缓存子项不重复计入总量", () => {
    const internal = { input: 3, output: 523, cache_write: 2057, cache_read: 160466 };
    expect(displayTokens(internal)).toEqual({
      input: 162526n,
      output: 523n,
      cache_write: 2057n,
      cache_read: 160466n,
    });
    expect(totalTokens(internal)).toBe(163049n);
    expect(internal.input).toBe(3);
  });

  it("Claude 与混合聚合采用相同口径", () => {
    const claude = { input: 10, output: 5, cache_write: 20, cache_read: 30 };
    expect(displayTokens(claude).input).toBe(60n);
    expect(totalTokens(claude)).toBe(65n);
    const mixed = { input: 13, output: 528, cache_write: 2077, cache_read: 160496 };
    expect(displayTokens(mixed).input).toBe(162586n);
    expect(totalTokens(mixed)).toBe(163114n);
  });

  it("超出 JS MAX_SAFE_INTEGER 与 u64::MAX 的字符串计数可无损求和、格式化并算比例", () => {
    const huge = {
      input: "18446744073709551615",
      output: "100000000000000000000",
      cache_write: "25",
      cache_read: "18446744073709551615",
    };
    const sum = totalTokens(huge);
    expect(sum).toBe(136893488147419103255n);
    expect(fmtNum(sum)).toBe("136,893,488,147,419,103,255");
    expect(ratioPercentBigInt(18446744073709551615n, displayTokens(huge).input)).toBeCloseTo(50, 2);
    expect(percentageFixed2BigInt(10n ** 350n, 4n * 10n ** 350n)).toBe("25.00");
    expect(bigIntToScaledNumber(12345n * 10n ** 306n, 20)).toBeCloseTo(1.2345e290, -280);
  });
});
