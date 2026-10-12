import { describe, expect, it } from "vitest";
import { displayTokens, totalTokens } from "./tokenUsage";

describe("总输入展示", () => {
  it("Codex 恢复日志总输入，缓存子项不重复计入总量", () => {
    const internal = { input: 3, output: 523, cache_write: 2057, cache_read: 160466 };
    expect(displayTokens(internal)).toEqual({ ...internal, input: 162526 });
    expect(totalTokens(internal)).toBe(163049);
    expect(internal.input).toBe(3);
  });
  it("Claude 与混合聚合采用相同口径", () => {
    const claude = { input: 10, output: 5, cache_write: 20, cache_read: 30 };
    expect(displayTokens(claude).input).toBe(60);
    expect(totalTokens(claude)).toBe(65);
    const mixed = { input: 13, output: 528, cache_write: 2077, cache_read: 160496 };
    expect(displayTokens(mixed).input).toBe(162586);
    expect(totalTokens(mixed)).toBe(163114);
  });
});
