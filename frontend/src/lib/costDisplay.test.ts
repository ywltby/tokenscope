// v0.9.3：费用显示状态纯函数——颜色与文案的单一来源。
// 后端金额与 unknown_tokens 不变，只验证「怎么显示」的五组状态。
import { describe, expect, it } from "vitest";
import { costDisplayText, costDisplayTone, unknownTokenTotal } from "./costDisplay";
import type { Group } from "../types";

function group(opts: Partial<Group> = {}): Group {
  return {
    key: "g",
    requests: 1,
    tokens: { input: 10, output: 5, cache_write: 0, cache_read: 0 },
    cost_usd: 0,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    ...opts,
  };
}

describe("费用显示状态（v0.9.3）", () => {
  it("部分未定价：橙色金额", () => {
    const g = group({
      total_cost_usd: 2,
      unknown_tokens: { input: 300, output: 0, cache_write: 0, cache_read: 0 },
    });
    expect(costDisplayTone(g)).toBe("warning");
    expect(costDisplayText(g, "未知")).toBe("$2.00");
  });

  it("全部未定价：橙色「未知」，不伪装成 $0.00", () => {
    const g = group({
      total_cost_usd: null,
      unknown_tokens: { input: 100, output: 0, cache_write: 0, cache_read: 0 },
    });
    expect(costDisplayTone(g)).toBe("warning");
    expect(costDisplayText(g, "未知")).toBe("未知");
  });

  it("全部已计价：普通文字色金额", () => {
    const g = group({ total_cost_usd: 8.25 });
    expect(costDisplayTone(g)).toBe("normal");
    expect(costDisplayText(g, "未知")).toBe("$8.25");
  });

  it("显式零价：普通文字色 $0.00", () => {
    const g = group({ total_cost_usd: 0, has_estimated_cost: true });
    expect(costDisplayTone(g)).toBe("normal");
    expect(costDisplayText(g, "未知")).toBe("$0.00");
  });

  it("仅按零估算标记、unknown=0：普通文字色", () => {
    const g = group({ total_cost_usd: 0, cache_price_zero_estimated: true });
    expect(unknownTokenTotal(g.unknown_tokens)).toBe(0);
    expect(costDisplayTone(g)).toBe("normal");
    expect(costDisplayText(g, "未知")).toBe("$0.00");
  });
});
